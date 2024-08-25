use core::str;
use std::io::{self, Write};

#[derive(Debug)]
pub enum Message {
    KeepAlive,
    Choke,
    UnChoke,
    Interested,
    NotInterested,
    Have {
        index: u32,
    },
    // TODO
    BitField,
    Request {
        index: u32,
        begin: u32,
        length: u32,
    },
    Cancel {
        index: u32,
        begin: u32,
        length: u32,
    },
    Piece {
        index: u32,
        begin: u32,
        piece: Vec<u8>,
    },
}

impl Message {
    pub fn as_bytes(&self) -> io::Result<Vec<u8>> {
        let mut buf = Vec::new();
        match self {
            Message::KeepAlive => {
                buf.write_all(&0u32.to_be_bytes())?;
            }
            Message::Choke => {
                buf.write_all(&1u32.to_be_bytes())?;
                buf.write_all(&[0])?;
            }
            Message::UnChoke => {
                buf.write_all(&1u32.to_be_bytes())?;
                buf.write_all(&[1])?;
            }
            Message::Interested => {
                buf.write_all(&1u32.to_be_bytes())?;
                buf.write_all(&[2])?;
            }
            Message::NotInterested => {
                buf.write_all(&1u32.to_be_bytes())?;
                buf.write_all(&[3])?;
            }
            Message::Have { index } => {
                buf.write_all(&5u32.to_be_bytes())?;
                buf.write_all(&[4])?;
                buf.write_all(&index.to_be_bytes())?;
            }
            Message::BitField => {
                todo!();
            }
            Message::Request {
                index,
                begin,
                length,
            } => {
                buf.write_all(&13u32.to_be_bytes())?;
                buf.write_all(&[6])?;
                buf.write_all(&index.to_be_bytes())?;
                buf.write_all(&begin.to_be_bytes())?;
                buf.write_all(&length.to_be_bytes())?;
            }
            Message::Piece {
                index,
                begin,
                piece,
            } => {
                buf.write_all(&(piece.len() as u32 + 9).to_be_bytes())?;
                buf.write_all(&[7])?;
                buf.write_all(&index.to_be_bytes())?;
                buf.write_all(&begin.to_be_bytes())?;
                buf.write_all(&piece)?;
            }
            Message::Cancel {
                index,
                begin,
                length,
            } => {
                buf.write_all(&13u32.to_be_bytes())?;
                buf.write_all(&[9])?;
                buf.write_all(&index.to_be_bytes())?;
                buf.write_all(&begin.to_be_bytes())?;
                buf.write_all(&length.to_be_bytes())?;
            }
        };
        Ok(buf)
    }
}

#[derive(Debug)]
pub struct HandShake {
    info_hash: [u8; 20],
    peer_id: [u8; 20],
}

impl HandShake {
    pub const BITTORRENT_PROTOCOL_STR: &'static str = "BitTorrent protocol";
    pub const BITTORRENT_PROTOCOL_BYTES: [u8; 19] = [
        66, 105, 116, 84, 111, 114, 114, 101, 110, 116, 32, 112, 114, 111, 116, 111, 99, 111, 108,
    ];

    pub fn new(info_hash: [u8; 20], peer_id: [u8; 20]) -> Self {
        Self { info_hash, peer_id }
    }

    pub fn as_bytes(&self) -> io::Result<Vec<u8>> {
        let mut buf = Vec::new();

        buf.write_all(&[19])?;
        buf.write_all(&Self::BITTORRENT_PROTOCOL_BYTES)?;
        buf.write_all(&[0u8; 8])?;
        buf.write_all(&self.info_hash)?;
        buf.write_all(&self.peer_id)?;

        Ok(buf)
    }
}
