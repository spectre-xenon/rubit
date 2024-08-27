use std::io::{Seek, SeekFrom};
use std::{
    collections::{HashSet, VecDeque},
    fs::File,
    io::{self, Read, Write},
    net::{SocketAddr, TcpStream},
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

use bencode::TorrentFile;
use sha1::{Digest, Sha1};

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
        file: Arc<Mutex<File>>,
    ) -> io::Result<()> {
        // connect or else remove address from peers HashSet
        let Ok(mut stream) = TcpStream::connect_timeout(&socket_addr, Duration::from_secs(2))
        else {
            let mut set = peers.lock().unwrap();
            set.remove(&socket_addr);
            return Ok(());
        };

        stream.set_read_timeout(Some(Duration::from_secs(2)))?;

        // println!("connected to peer {}", socket_addr);

        let mut peer_pieces = HashSet::new();

        {
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
                        5 => {
                            self.read_bitfield(buf, &mut peer_pieces);
                        }
                        4 => {
                            self.read_have(buf, &mut peer_pieces);
                        }
                        1 => {
                            self.state = State::UnChoked;
                            break;
                        }
                        _ => break,
                    },
                    Err(_) => {
                        break;
                    }
                }
            }

            thread::sleep(Duration::from_millis(1));
        }

        stream.set_read_timeout(None)?;

        loop {
            if self.my_state == State::None {
                stream.write(&Message::Interested.as_bytes()?)?;
                self.my_state = State::Interested;
            }

            if self.state == State::Choked {
                loop {
                    let buf = self.read_stream(&mut stream)?;
                    // println!("got unchoke!");
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
                    None => {
                        // println!("empty queue! returing..");
                        let mut set = peers.lock().unwrap();
                        set.remove(&socket_addr);
                        stream.write(&Message::NotInterested.as_bytes()?)?;
                        return Ok(());
                    }
                };

                if !peer_pieces.contains(&piece_index) {
                    queue.push_back(piece_index);
                    std::mem::drop(queue);
                    thread::sleep(Duration::from_millis(1));
                    continue;
                }

                peer_pieces.remove(&piece_index);

                std::mem::drop(queue);

                let piece_len = if piece_index == torrent_file.info.pieces.len() - 1
                    && torrent_file.info.length % torrent_file.info.piece_length != 0
                {
                    (torrent_file.info.length % torrent_file.info.piece_length) as usize
                } else {
                    torrent_file.info.piece_length as usize
                };

                let block_len = match piece_len {
                    n if n < 16384 => piece_len,
                    _ => 16384,
                };

                let num_blocks = if piece_len % block_len == 0 {
                    (piece_len / block_len) as usize
                } else {
                    (piece_len as f64 / block_len as f64).ceil() as usize
                };

                let mut buf: Vec<u8> = Vec::new();
                let mut hasher = Sha1::new();

                for i in 0..num_blocks {
                    let len = if i == num_blocks - 1 && piece_len % block_len != 0 {
                        piece_len % block_len
                    } else {
                        block_len
                    };

                    stream.write(
                        &Message::Request {
                            index: piece_index as u32,
                            begin: (i * block_len) as u32,
                            length: len as u32,
                        }
                        .as_bytes()?,
                    )?;
                    loop {
                        let block = self.read_stream(&mut stream)?;
                        if block[0] == 7 {
                            buf.write_all(&block[9..])?;
                            hasher.update(&block[9..]);
                            // println!("got block {} from {}", i, socket_addr);
                            break;
                        } else if block[0] == 0 {
                            self.state = State::Choked;
                            self.push_back_to_queue(&global_queue, &mut peer_pieces, piece_index);
                            break;
                        }
                        thread::sleep(Duration::from_millis(1));
                    }
                }

                let hash: [u8; 20] = hasher.finalize().into();

                // println!("rec hash: {:?}", hash);
                // println!("org hash: {:?}", torrent_file.info.pieces[piece_index]);

                if torrent_file.info.pieces[piece_index] == hash {
                    let mut file = file.lock().unwrap();
                    file.seek(SeekFrom::Start(
                        piece_index as u64 * torrent_file.info.piece_length as u64,
                    ))?;
                    file.write(&buf)?;

                    std::mem::drop(file);

                    // println!("wrote piece {} to disk!", piece_index);
                } else {
                    self.push_back_to_queue(&global_queue, &mut peer_pieces, piece_index);
                }
                thread::sleep(Duration::from_millis(1));
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

    fn read_stream(&self, stream: &mut impl Read) -> io::Result<Vec<u8>> {
        #[allow(unused_assignments)]
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
            return Ok(vec![9]);
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

    fn push_back_to_queue(
        &self,
        queue: &Arc<Mutex<VecDeque<usize>>>,
        peer_pieces: &mut HashSet<usize>,
        value: usize,
    ) {
        let mut queue = queue.lock().unwrap();
        queue.push_back(value);
        peer_pieces.insert(value);
        std::mem::drop(queue);
    }
}
