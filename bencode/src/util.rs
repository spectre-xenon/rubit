use std::collections::HashMap;

use sha1::{Digest, Sha1};

use crate::{BencodeTypes, ParseError, Peers};

pub fn get_hash(slice: &[u8]) -> Result<[u8; 20], ParseError> {
    let mut hasher = Sha1::new();
    hasher.update(slice);
    Ok(hasher.finalize().into())
}

pub fn unwrap_string(string: BencodeTypes) -> Option<String> {
    if let BencodeTypes::String(s) = string {
        Some(s)
    } else {
        None
    }
}

pub fn unwrap_integer(int: BencodeTypes) -> Option<u64> {
    if let BencodeTypes::Integer(i) = int {
        Some(i)
    } else {
        None
    }
}

pub fn unwrap_announce_list(vec: BencodeTypes) -> Option<Vec<Vec<String>>> {
    let BencodeTypes::List(vec) = vec else {
        return None;
    };

    let out = vec
        .iter()
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
        .collect();

    Some(out)
}

pub fn unwrap_dict(dict: BencodeTypes) -> Option<HashMap<String, BencodeTypes>> {
    if let BencodeTypes::Dict(d) = dict {
        Some(d)
    } else {
        None
    }
}

pub fn unwrap_info_hash(info_hash: BencodeTypes) -> Option<[u8; 20]> {
    if let BencodeTypes::InfoHash(ih) = info_hash {
        Some(ih)
    } else {
        None
    }
}

pub fn unwrap_pieces(pieces: BencodeTypes) -> Option<Vec<[u8; 20]>> {
    if let BencodeTypes::Pieces(p) = pieces {
        Some(p)
    } else {
        None
    }
}

fn parse_ip(string: String) -> (u8, u8, u8, u8) {
    let vec: Vec<_> = string
        .split(".")
        .map(|s| s.parse::<u8>().expect("failed to parse ip"))
        .collect();
    (vec[0], vec[1], vec[2], vec[3])
}

pub fn unwrap_peers(peers: BencodeTypes) -> Option<Peers> {
    match peers {
        BencodeTypes::PeersCompact(p) => Some(p),
        BencodeTypes::List(mut l) => Some(
            l.iter_mut()
                .map(|item| match item {
                    BencodeTypes::Dict(d) => {
                        let ip = parse_ip(
                            unwrap_string(d.remove("ip").expect("no ip param found"))
                                .expect("failed to get ip"),
                        );

                        let port = unwrap_integer(d.remove("port").expect("no portW param found"))
                            .expect("failed to get port");

                        (ip, port as u16)
                    }
                    _ => panic!("not a List"),
                })
                .collect(),
        ),
        _ => unimplemented!(),
    }
}
