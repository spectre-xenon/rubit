use std::io::{Seek, SeekFrom};
use std::{
    collections::{HashSet, VecDeque},
    fs::File,
    io::{self, Read, Write},
    net::{SocketAddr, TcpStream},
    sync::{Arc, Mutex},
    time::Duration,
};

use rubit_bencode::TorrentFile;
use sha1::{Digest, Sha1};

use crate::{HandShake, Message};

pub enum ConnError {
    Io(io::Error),
    TimeOut,
    EmptyQueue,
}

impl From<io::Error> for ConnError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

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
        socket_addr: SocketAddr,
        torrent_file: Arc<TorrentFile>,
        peer_id: [u8; 20],
        file: Arc<Mutex<File>>,
        verbose: bool,
    ) -> Result<(), ConnError> {
        // connect or else remove address from peers HashSet
        let Ok(mut stream) = TcpStream::connect_timeout(&socket_addr, Duration::from_secs(2))
        else {
            return Err(ConnError::TimeOut);
        };

        // Handshake timeout
        stream.set_read_timeout(Some(Duration::from_secs(2)))?;

        if verbose {
            println!("connected to peer {}", socket_addr);
        }

        let mut peer_pieces = HashSet::new();

        {
            let handshake_bytes = HandShake::new(torrent_file.info_hash, peer_id).as_bytes()?;

            stream.write_all(&handshake_bytes)?;

            // Size of handshake = 68 bytes
            let mut handshake_buf = [0u8; 68];
            stream.read_exact(&mut handshake_buf)?;

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
        }

        // Normal timeout: 10Secs may be too long :/
        stream.set_read_timeout(Some(Duration::from_secs(10)))?;

        loop {
            if self.my_state == State::None {
                stream.write_all(&Message::Interested.as_bytes()?)?;
                self.my_state = State::Interested;
            }

            if self.state == State::Choked {
                let buf = self.read_stream(&mut stream)?;
                if verbose {
                    println!("got unchoke!");
                }
                if buf[0] == 1 {
                    self.state = State::UnChoked;
                }
            }

            if self.state == State::UnChoked {
                let mut queue = global_queue.lock().unwrap();
                let piece_index = match queue.pop_front() {
                    Some(i) => i,
                    None => {
                        if verbose {
                            println!("empty queue! returing..");
                        }
                        stream.write_all(&Message::NotInterested.as_bytes()?)?;
                        return Err(ConnError::EmptyQueue);
                    }
                };

                if !peer_pieces.contains(&piece_index) {
                    queue.push_back(piece_index);
                    continue;
                }

                peer_pieces.remove(&piece_index);

                // Release lock on queue
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

                    stream.write_all(
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
                            buf.extend_from_slice(&block[9..]);
                            hasher.update(&block[9..]);
                            if verbose {
                                println!("got block {} from {}", i, socket_addr);
                            }
                            break;
                        } else if block[0] == 0 {
                            self.state = State::Choked;
                            self.push_back_to_queue(&global_queue, &mut peer_pieces, piece_index);
                            break;
                        }
                    }
                }

                let hash: [u8; 20] = hasher.finalize().into();

                if verbose {
                    println!("recivied hash: {:?}", hash);
                    println!("original hash: {:?}", torrent_file.info.pieces[piece_index]);
                }

                if torrent_file.info.pieces[piece_index] == hash {
                    let mut file = file.lock().unwrap();
                    file.seek(SeekFrom::Start(
                        piece_index as u64 * torrent_file.info.piece_length as u64,
                    ))?;
                    file.write_all(&buf)?;

                    std::mem::drop(file);

                    if verbose {
                        println!("wrote piece {} to disk!", piece_index);
                    }
                } else {
                    self.push_back_to_queue(&global_queue, &mut peer_pieces, piece_index);
                }
            }
        }
    }

    fn read_bitfield(&self, buf: Vec<u8>, peer_pieces: &mut HashSet<usize>) {
        let mut pointer = 0usize;
        for index in 1..buf.len() {
            for bit in 0..8 {
                let mask = 1 << (7 - bit);
                let bit_is_set = (mask & buf[index]) != 0;
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
        let mut len_prefix = [0; 4];
        stream.read_exact(&mut len_prefix)?;
        let num = u32::from_be_bytes(len_prefix) as usize;

        // Empty message = Keepalive
        if num == 0 {
            return Ok(Vec::from([9]));
        }

        let mut buf = Vec::new();
        buf.resize(num as usize, 0);
        stream.read_exact(&mut buf)?;

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
