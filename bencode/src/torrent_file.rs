use core::fmt;
use std::{collections::HashMap, process::exit};

use crate::decode::{decode_dict, BencodeTypes};

pub struct Info {
    pub name: String,
    pub length: u32,
    pub piece_length: u32,
    pub pieces: Vec<[u8; 20]>,
}

pub struct TorrentFile {
    pub info_hash: [u8; 20],
    pub announce: String,
    pub announce_list: Vec<Vec<String>>,
    pub info: Info,
}

fn unwrap_announce_vec(vec: Vec<BencodeTypes>) -> Vec<Vec<String>> {
    vec.iter()
        .map(|item| match item {
            BencodeTypes::List(v) => v
                .iter()
                .map(|item| match item {
                    BencodeTypes::String(s) => s.clone(),
                    _ => panic!("not a String"),
                })
                .collect(),
            _ => panic!("not a List"),
        })
        .collect()
}

fn make_torrent_file<'a>(dict: &'a mut HashMap<String, BencodeTypes>) -> Option<TorrentFile> {
    let BencodeTypes::InfoHash(info_hash) = dict.remove("info_hash")? else {
        return None;
    };
    let BencodeTypes::String(announce) = dict.remove("announce")? else {
        return None;
    };

    let BencodeTypes::Dict(mut info_dict) = dict.remove("info")? else {
        return None;
    };
    let BencodeTypes::String(name) = info_dict.remove("name")? else {
        return None;
    };
    let BencodeTypes::Integer(length) = info_dict.remove("length")? else {
        return None;
    };
    let BencodeTypes::Integer(piece_length) = info_dict.remove("piece length")? else {
        return None;
    };
    let BencodeTypes::Pieces(pieces) = info_dict.remove("pieces")? else {
        return None;
    };
    let info = Info {
        name,
        length,
        piece_length,
        pieces,
    };

    let BencodeTypes::List(temp_announce_list) = dict.remove("announce-list")? else {
        return None;
    };

    let announce_list = unwrap_announce_vec(temp_announce_list);

    Some(TorrentFile {
        info_hash,
        announce,
        announce_list,
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
