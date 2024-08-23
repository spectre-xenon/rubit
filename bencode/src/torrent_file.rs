use core::fmt;
use std::{collections::HashMap, process::exit};

use crate::{
    decode::{decode_dict, BencodeTypes},
    unwrap_announce_list, unwrap_dict, unwrap_info_hash, unwrap_integer, unwrap_pieces,
    unwrap_string,
};

pub struct Info {
    pub name: String,
    pub length: u32,
    pub piece_length: u32,
    pub pieces: Vec<[u8; 20]>,
}

pub struct TorrentFile {
    pub info_hash: [u8; 20],
    pub announce: String,
    pub announce_list: Option<Vec<Vec<String>>>,
    pub created_by: Option<String>,
    pub creation_date: Option<u32>,
    pub encoding: Option<String>,
    pub info: Info,
}

fn make_torrent_file<'a>(dict: &'a mut HashMap<String, BencodeTypes>) -> Option<TorrentFile> {
    let info_hash = unwrap_info_hash(dict.remove("info_hash")?)?;
    let announce = unwrap_string(dict.remove("announce")?)?;
    let mut info_dict = unwrap_dict(dict.remove("info")?)?;

    let name = unwrap_string(info_dict.remove("name")?)?;
    let length = unwrap_integer(info_dict.remove("length")?)?;
    let piece_length = unwrap_integer(info_dict.remove("piece length")?)?;
    let pieces = unwrap_pieces(info_dict.remove("pieces")?)?;

    let info = Info {
        name,
        length,
        piece_length,
        pieces,
    };

    let announce_list = match dict.remove("announce-list") {
        Some(l) => unwrap_announce_list(l),
        None => None,
    };

    let created_by = match dict.remove("created by") {
        Some(s) => unwrap_string(s),
        None => None,
    };

    let creation_date = match dict.remove("creation date") {
        Some(i) => unwrap_integer(i),
        None => None,
    };

    let encoding = match dict.remove("encoding") {
        Some(s) => unwrap_string(s),
        None => None,
    };

    Some(TorrentFile {
        info_hash,
        announce,
        announce_list,
        created_by,
        creation_date,
        encoding,
        info,
    })
}

impl From<Vec<u8>> for TorrentFile {
    fn from(buf: Vec<u8>) -> Self {
        let mut pointer = 0;
        let mut dict = match decode_dict(&mut pointer, &buf) {
            Ok(d) => d,
            Err(e) => {
                println!("bad torrent file!: {e:?}");
                exit(1);
            }
        };

        match make_torrent_file(&mut dict) {
            Some(t) => t,
            None => {
                println!("Bad torrent file!");
                println!("Is it a single-file torrent?");
                exit(1);
            }
        }
    }
}

impl fmt::Debug for TorrentFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TorrentFile")
            .field("info_hash", &self.info_hash)
            .field("announce", &self.announce)
            .field("announce-list", &self.announce_list)
            .field("info", &self.info)
            .finish()
    }
}

impl fmt::Debug for Info {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TorrentFile")
            .field("name", &self.name)
            .field("length", &self.length)
            .field("piece length", &self.piece_length)
            .field("pieces", &format!("too much to show!"))
            .finish()
    }
}
