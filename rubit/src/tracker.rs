use std::{
    array::TryFromSliceError,
    borrow::Cow,
    io::{self, Write},
    net::{SocketAddr, UdpSocket},
    time::Duration,
};

use rand::{random, thread_rng, Rng};
use rubit_bencode::{decode_dict, unwrap_integer, unwrap_peers, unwrap_string, Peers};
use url::{form_urlencoded, Url};

#[derive(Debug)]
pub enum TrackerError {
    Bencode(rubit_bencode::ParseError),
    Http(ureq::Error),
    Io(io::Error),
    Slice(TryFromSliceError),
    FailedDecode,
    NotHttpsAble,
    UnknownTrackerProtocol,
    MissMatchAction,
    MissMatchTransactionId,
}

impl From<rubit_bencode::ParseError> for TrackerError {
    fn from(value: rubit_bencode::ParseError) -> Self {
        Self::Bencode(value)
    }
}

impl From<ureq::Error> for TrackerError {
    fn from(value: ureq::Error) -> Self {
        Self::Http(value)
    }
}

impl From<io::Error> for TrackerError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<TryFromSliceError> for TrackerError {
    fn from(value: TryFromSliceError) -> Self {
        Self::Slice(value)
    }
}

#[derive(Debug)]
pub struct AnnounceConfig {
    pub info_hash: [u8; 20],
    pub peer_id: String,
    pub port: u16,
    pub uploaded: u64,
    pub downloaded: u64,
    pub left: u64,
}

#[derive(Debug)]
pub enum Responses {
    Failure(FailureResponse),
    Done(OkResponse),
}

#[derive(Debug)]
pub struct FailureResponse {
    pub failure_reason: String,
}

#[derive(Debug)]
pub struct OkResponse {
    pub interval: Duration,
    pub min_interval: Option<Duration>,
    /// Seeders number
    pub complete: Option<u64>,
    /// Leechers number
    pub incomplete: Option<u64>,
    pub peers: Peers,
}

#[derive(Debug, PartialEq, Eq)]
pub enum UrlProtocol {
    UDP,
    HTTP,
}

#[derive(Debug)]
pub struct Tracker {
    pub url: Url,
    pub protocol: UrlProtocol,
}

impl Tracker {
    const UDP_MAGIC_CONSTANT: u64 = 0x41727101980;

    pub fn new(url: Url) -> Result<Self, TrackerError> {
        let protocol = match url.scheme() {
            "http" => UrlProtocol::HTTP,
            "https" => UrlProtocol::HTTP,
            "udp" => UrlProtocol::UDP,
            _ => panic!("unknown tracker protocl!"),
        };

        Ok(Self { url, protocol })
    }

    pub fn announce(&self, config: AnnounceConfig) -> Result<Responses, TrackerError> {
        match self.protocol {
            UrlProtocol::HTTP => self.announce_http(config),
            UrlProtocol::UDP => self.announce_udp(config),
        }
    }

    fn decode_http_response(&self, response: Vec<u8>) -> Option<Responses> {
        let mut pointer = 0;
        let mut dict = decode_dict(&mut pointer, &response).expect("failed to decode response");

        if dict.contains_key("failure reason") {
            let failure_reason = unwrap_string(dict.remove("failure reason")?)?;
            return Some(Responses::Failure(FailureResponse { failure_reason }));
        }

        let interval = Duration::from_secs(unwrap_integer(dict.remove("interval")?)?);

        let min_interval = match dict.remove("min interval") {
            Some(i) => Some(Duration::from_secs(unwrap_integer(i)?)),
            None => None,
        };

        let complete = match dict.remove("complete") {
            Some(i) => unwrap_integer(i),
            None => None,
        };

        let incomplete = match dict.remove("incomplete") {
            Some(i) => unwrap_integer(i),
            None => None,
        };

        let peers = unwrap_peers(dict.remove("peers")?)?;

        Some(Responses::Done(OkResponse {
            interval,
            min_interval,
            complete,
            incomplete,
            peers,
        }))
    }

    fn announce_http(&self, config: AnnounceConfig) -> Result<Responses, TrackerError> {
        // necessary get request params
        let params = form_urlencoded::Serializer::new(String::new())
            .append_pair("peer_id", &config.peer_id)
            .append_pair("port", &config.port.to_string())
            .append_pair("left", &config.left.to_string())
            .append_pair("uploaded", &config.uploaded.to_string())
            .append_pair("downloaded", &config.downloaded.to_string())
            .append_pair("compact", "1")
            // a hack to convert info hash to its encoded form needed in:
            // https://www.bittorrent.org/beps/bep_0003.html
            .encoding_override(Some(&|input| {
                if input != "!" {
                    Cow::Borrowed(input.as_bytes())
                } else {
                    Cow::Owned(config.info_hash.to_vec())
                }
            }))
            .append_pair("info_hash", "!")
            .finish();

        // get request
        let mut response_buf = Vec::new();
        ureq::get(&format!("{}?{}", &self.url.to_string(), params))
            .call()?
            .into_reader()
            .read_to_end(&mut response_buf)?;

        // decode the bencode response
        match self.decode_http_response(response_buf) {
            None => Err(TrackerError::FailedDecode),
            Some(e) => Ok(e),
        }
    }

    fn connect_udp(&self, receiver_ip: SocketAddr) -> Result<u64, TrackerError> {
        let socket = UdpSocket::bind("0.0.0.0:0")?;

        let transaction_id: u32 = random();

        let mut write_buf = Vec::new();
        write_buf.write_all(&Self::UDP_MAGIC_CONSTANT.to_be_bytes())?;
        // action: 0 = connect
        write_buf.write_all(&0u32.to_be_bytes())?;
        write_buf.write_all(&transaction_id.to_be_bytes())?;

        socket.send_to(&write_buf, receiver_ip)?;

        let mut rec_buf = [0u8; 2048];
        socket.recv_from(&mut rec_buf)?;

        let action = u32::from_be_bytes(rec_buf[0..4].try_into()?);
        let rec_transaction_id = u32::from_be_bytes(rec_buf[4..8].try_into()?);
        let connection_id = u64::from_be_bytes(rec_buf[8..16].try_into()?);

        if action != 0 {
            return Err(TrackerError::MissMatchAction);
        }
        if rec_transaction_id != transaction_id {
            return Err(TrackerError::MissMatchTransactionId);
        }

        Ok(connection_id)
    }

    /// https://www.bittorrent.org/beps/bep_0015.html
    fn announce_udp(&self, config: AnnounceConfig) -> Result<Responses, TrackerError> {
        let receiver_ip = self.url.socket_addrs(|| None)?[0];

        let port = thread_rng().gen_range(1025..u16::MAX);
        let socket = UdpSocket::bind(format!("0.0.0.0:{}", port))?;

        let connection_id = self.connect_udp(receiver_ip)?;
        let transaction_id: u32 = random();
        let key: u32 = random();

        let mut write_buf = Vec::new();
        write_buf.write_all(&connection_id.to_be_bytes())?;
        // Action: 1 = announce
        write_buf.write_all(&1u32.to_be_bytes())?;
        write_buf.write_all(&transaction_id.to_be_bytes())?;
        write_buf.write_all(&config.info_hash)?;
        write_buf.write_all(&config.peer_id.as_bytes())?;
        write_buf.write_all(&config.downloaded.to_be_bytes())?;
        write_buf.write_all(&config.left.to_be_bytes())?;
        write_buf.write_all(&config.uploaded.to_be_bytes())?;
        // Event: 0 = None
        write_buf.write_all(&0u32.to_be_bytes())?;
        // Ip Adress: 0 = default
        // Is specified in certian cases when the client is behind some kind of proxy
        write_buf.write_all(&0u32.to_be_bytes())?;
        write_buf.write_all(&key.to_be_bytes())?;
        // num_want: -1 = default
        // Specifies the number of peers to return -1 means as much as u can
        write_buf.write_all(&i32::from(-1).to_be_bytes())?;
        write_buf.write_all(&config.port.to_be_bytes())?;

        socket.send_to(&write_buf, receiver_ip)?;

        let mut rec_buf = [0u8; 2048];
        socket.recv_from(&mut rec_buf)?;

        if rec_buf == [0u8; 2048] {}

        let rec_action = u32::from_be_bytes(rec_buf[0..4].try_into()?);
        let rec_transaction_id = u32::from_be_bytes(rec_buf[4..8].try_into()?);
        let interval = Duration::from_secs(u32::from_be_bytes(rec_buf[8..12].try_into()?) as u64);
        let incomplete = Some(u32::from_be_bytes(rec_buf[12..16].try_into()?) as u64);
        let complete = Some(u32::from_be_bytes(rec_buf[16..20].try_into()?) as u64);
        let peers: Peers = rec_buf[20..]
            .chunks(6)
            .filter(|chunk| *chunk != [0u8; 6])
            .map(|chunk| {
                (
                    (chunk[0], chunk[1], chunk[2], chunk[3]),
                    u16::from_be_bytes([chunk[4], chunk[5]]),
                )
            })
            .collect();

        if rec_action != 1 {
            return Err(TrackerError::MissMatchAction);
        }
        if rec_transaction_id != transaction_id {
            return Err(TrackerError::MissMatchTransactionId);
        }

        Ok(Responses::Done(OkResponse {
            interval,
            min_interval: None,
            complete,
            incomplete,
            peers,
        }))
    }
}
