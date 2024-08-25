use std::{
    collections::{HashSet, VecDeque},
    io::{self, Read, Write},
    net::{SocketAddr, TcpStream},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use bencode::TorrentFile;
use openssl::sha::Sha1;

use crate::{HandShake, Message};

#[derive(PartialEq)]
pub enum State {
    Choked,
    UnChoked,
    Interested,
    Notinterested,
    None,
}

pub struct PeerConnManager {
    my_state: State,
    state: State,
}

impl PeerConnManager {
    pub fn new() -> Self {
        Self {
            my_state: State::None,
            state: State::Choked,
        }
    }

    pub fn handle_peer(
        &mut self,
        global_queue: Arc<Mutex<VecDeque<usize>>>,
        peers: Arc<Mutex<HashSet<SocketAddr>>>,
        socket_addr: SocketAddr,
        torrent_file: Arc<TorrentFile>,
        peer_id: [u8; 20],
    ) -> io::Result<()> {
        // connect or else remove address from peers HashSet
        let Ok(mut stream) = TcpStream::connect_timeout(&socket_addr, Duration::from_secs(2))
        else {
            let mut set = peers.lock().unwrap();
            set.remove(&socket_addr);
            return Ok(());
        };

        stream.set_read_timeout(Some(Duration::from_secs(1)))?;

        println!("connected to peer {}", socket_addr);

        let mut is_handshake = true;
        let mut peer_pieces = HashSet::new();

        loop {
            if is_handshake {
                let handshake_bytes = HandShake::new(torrent_file.info_hash, peer_id).as_bytes()?;

                stream.write(&handshake_bytes)?;

                // Size of handshake = 68 bytes
                let mut handshake_buf = [0u8; 68];

                loop {
                    stream.read_exact(&mut handshake_buf)?;
                    if handshake_buf != [0u8; 68] {
                        break;
                    }
                }

                if handshake_bytes[28..48] != handshake_buf[28..48] {
                    return Ok(());
                }

                // listen until choke message
                loop {
                    match self.read_stream(&mut stream) {
                        Ok(buf) => match buf[0] {
                            5 => self.read_bitfield(buf, &mut peer_pieces),
                            4 => self.read_have(buf, &mut peer_pieces),
                            _ => break,
                        },
                        Err(_) => {
                            break;
                        }
                    }
                }
                is_handshake = false;
                thread::sleep(Duration::from_millis(1));
            }

            if self.my_state == State::None {
                stream.write(&Message::Interested.as_bytes()?)?;
                self.my_state = State::Interested;
            }

            if self.state == State::Choked {
                loop {
                    let buf = self.read_stream(&mut stream)?;
                    if buf[0] == 1 {
                        self.state = State::UnChoked;
                        break;
                    }
                }
            }

            if self.state == State::UnChoked {
                let mut queue = global_queue.lock().unwrap();
                let piece_index = match queue.pop_front() {
                    Some(i) => i,
                    None => return Ok(()),
                };

                if !peer_pieces.contains(&piece_index) {
                    queue.push_back(piece_index);
                    continue;
                }

                std::mem::drop(queue);

                let blocks = torrent_file.info.piece_length / 16384;

                let mut buf: Vec<u8> = Vec::new();
                buf.resize(torrent_file.info.piece_length as usize, 0);

                let mut hasher = Sha1::new();
                for i in 0..blocks {
                    stream.write(
                        &Message::Request {
                            index: piece_index as u32,
                            begin: (i * 16384) as u32,
                            length: 16384,
                        }
                        .as_bytes()?,
                    )?;

                    let block = self.read_stream(&mut stream)?;
                    if block[0] == 7 {
                        let rec_begin = u32::from_be_bytes(block[5..9].try_into().unwrap());
                        // println!("curr begin: {}", i * 16384);

                        // println!("rec begin: {}", rec_begin);
                        buf.write_all(&block[9..])?;
                        hasher.update(&block[9..]);
                        println!("got block num {} from {}", i, socket_addr);
                        thread::sleep(Duration::from_millis(1))
                    }
                }

                let hash = hasher.finish();

                println!("org_hash: {:?}", torrent_file.info.pieces[piece_index]);
                println!("rec_hash: {:?}", hash);
            }
        }
    }

    fn read_bitfield(&self, buf: Vec<u8>, peer_pieces: &mut HashSet<usize>) {
        let mut pointer = 0usize;
        for index in 1..buf.len() {
            for bit in 0..8 {
                let mask = 255 >> bit;
                let bit_is_set = (mask & buf[index]) > 0;
                if bit_is_set {
                    peer_pieces.insert(pointer);
                }
                pointer += 1;
            }
        }
    }

    fn read_have(&self, buf: Vec<u8>, peer_pieces: &mut HashSet<usize>) {
        peer_pieces.insert(u32::from_be_bytes(buf[1..5].try_into().unwrap()) as usize);
    }

    fn read_stream(&self, stream: &mut TcpStream) -> io::Result<Vec<u8>> {
        let mut len_prefix2 = [0; 4];

        loop {
            let mut len_prefix = [0; 4];
            stream.read_exact(&mut len_prefix)?;
            if len_prefix.len() > 0 && len_prefix.len() == 4 && u32::from_be_bytes(len_prefix) != 0
            {
                len_prefix2 = len_prefix;
                break;
            }
        }
        let num = u32::from_be_bytes(len_prefix2) as usize;

        if num == 0 {
            return Ok(vec![0]);
        }

        let mut buf = Vec::new();
        buf.resize(num as usize, 0);

        loop {
            if buf.len() > 0 && buf.len() >= num {
                stream.read_exact(&mut buf)?;
                break;
            }
        }
        Ok(buf)
    }
}
