use core::str;
use std::{collections::HashMap, process::exit};

use sha1::{Digest, Sha1};

use crate::errors::ParseError;

const INTEGER_START: u8 = 0x69; // 'i'
const STRING_DELIM: u8 = 0x3A; // ':'
const DICTIONARY_START: u8 = 0x64; // 'd'
const LIST_START: u8 = 0x6C; // 'l'
const END_OF_TYPE: u8 = 0x65; // 'e'

pub type Peers = Vec<((u8, u8, u8, u8), u16)>;

#[derive(Debug, PartialEq)]
pub enum BencodeTypes {
    String(String),
    Integer(u32),
    List(Vec<BencodeTypes>),
    Dict(HashMap<String, BencodeTypes>),
    InfoHash([u8; 20]),
    Pieces(Vec<[u8; 20]>),
    PeersCompact(Peers),
}

fn parse_to_utf8(slice: &[u8]) -> Result<String, ParseError> {
    let string_slice = str::from_utf8(slice)?;
    Ok(string_slice.to_string())
}

fn parse_to_usize(slice: &[u8]) -> Result<usize, ParseError> {
    let string = parse_to_utf8(slice)?;
    Ok(string.parse()?)
}

fn get_string_len(pointer: &mut usize, buf: &Vec<u8>) -> Result<usize, ParseError> {
    let mut temp = Vec::new();

    // Increment and push to vec until delim
    while buf[*pointer] != STRING_DELIM {
        temp.push(buf[*pointer]);
        *pointer += 1;
    }

    // Place pointer on the start of the string (after delim)
    *pointer += 1;

    if temp.len() == 1 && temp[0] == 48 {
        return Ok(0);
    }

    Ok(parse_to_usize(&temp)?)
}

pub fn decode_string(pointer: &mut usize, buf: &Vec<u8>) -> Result<String, ParseError> {
    let string_len = get_string_len(pointer, buf)?;

    if string_len == 0 {
        return Ok("".to_string());
    }

    let string_len = string_len + *pointer;
    let slice: &[u8] = &buf[*pointer..string_len];

    // Place pointer at the byte after the string (after the last char)
    *pointer = string_len;

    Ok(parse_to_utf8(slice)?)
}

pub fn decode_int(pointer: &mut usize, buf: &Vec<u8>) -> Result<u32, ParseError> {
    let mut int_bytes = Vec::new();

    // Place pointer at start of int (after "i")
    *pointer += 1;

    while buf[*pointer] != END_OF_TYPE {
        int_bytes.push(buf[*pointer]);
        *pointer += 1;
    }

    // Place pointer at end of type (after "e")
    *pointer += 1;

    Ok(parse_to_usize(&int_bytes)? as u32)
}

pub fn decode_list(pointer: &mut usize, buf: &Vec<u8>) -> Result<Vec<BencodeTypes>, ParseError> {
    let mut list: Vec<BencodeTypes> = Vec::new();

    // Place pointer at start of list (after "l")
    *pointer += 1;

    while buf[*pointer] != END_OF_TYPE {
        list.push(match buf[*pointer] {
            n if n.is_ascii_digit() => BencodeTypes::String(decode_string(pointer, buf)?),
            INTEGER_START => BencodeTypes::Integer(decode_int(pointer, buf)?),
            LIST_START => BencodeTypes::List(decode_list(pointer, buf)?),
            DICTIONARY_START => BencodeTypes::Dict(decode_dict(pointer, buf)?),
            _ => todo!(),
        })
    }

    // Place pointer at end of type (after "e")
    *pointer += 1;

    Ok(list)
}

pub fn decode_pieces(pointer: &mut usize, buf: &Vec<u8>) -> Result<Vec<[u8; 20]>, ParseError> {
    let pieces_len = get_string_len(pointer, buf)?;

    let pieces_len = pieces_len + *pointer;

    let pieces_vec: Vec<[u8; 20]> = buf[*pointer..pieces_len]
        .chunks_exact(20)
        .map(|h| match h.try_into() {
            Ok(h) => h,
            Err(e) => {
                println!("bad pieces array! e: {e}");
                exit(1);
            }
        })
        .collect();

    // Place pointer at the byte after the string (after the last char)
    *pointer = pieces_len;

    Ok(pieces_vec)
}

fn decode_peers(pointer: &mut usize, buf: &Vec<u8>) -> Result<BencodeTypes, ParseError> {
    if buf[*pointer] == LIST_START {
        let decoded = decode_list(pointer, buf)?;
        return Ok(BencodeTypes::List(decoded));
    }

    let peers_len = get_string_len(pointer, buf)?;

    let peers_len = peers_len + *pointer;

    let peers_vec = buf[*pointer..peers_len]
        .chunks_exact(6)
        .map(|item| {
            (
                (item[0], item[1], item[2], item[3]),
                u16::from_be_bytes([item[4], item[5]]),
            )
        })
        .collect();

    // Place pointer at the byte after the string (after the last char)
    *pointer = peers_len;

    Ok(BencodeTypes::PeersCompact(peers_vec))
}

fn get_hash(slice: &[u8]) -> Result<[u8; 20], ParseError> {
    let mut hasher = Sha1::new();
    hasher.update(slice);
    Ok(hasher.finalize().into())
}

pub fn decode_dict(
    pointer: &mut usize,
    buf: &Vec<u8>,
) -> Result<HashMap<String, BencodeTypes>, ParseError> {
    if buf.len() == 0 {
        return Err(ParseError::BadFile);
    }

    let mut dict: HashMap<String, BencodeTypes> = HashMap::new();

    // Place pointer at start of dict (after "d")
    *pointer += 1;

    let mut is_key = true;
    let mut temp_key = String::new();
    let mut info_hash_start: usize = 0;

    while buf[*pointer] != END_OF_TYPE && *pointer != buf.len() {
        if is_key {
            temp_key = decode_string(pointer, buf)?;
            if temp_key == "info" {
                info_hash_start = *pointer;
            }
            is_key = !is_key;
            continue;
        }

        let parsed = match buf[*pointer] {
            n if n.is_ascii_digit() && temp_key == "pieces" => {
                BencodeTypes::Pieces(decode_pieces(pointer, buf)?)
            }
            n if n.is_ascii_digit() && temp_key == "peers" => decode_peers(pointer, buf)?,
            n if n.is_ascii_digit() => BencodeTypes::String(decode_string(pointer, buf)?),
            INTEGER_START => BencodeTypes::Integer(decode_int(pointer, buf)?),
            LIST_START => BencodeTypes::List(decode_list(pointer, buf)?),
            DICTIONARY_START => BencodeTypes::Dict(decode_dict(pointer, buf)?),
            _ => todo!(),
        };

        dict.insert(temp_key.clone(), parsed);
        is_key = !is_key;
    }

    // info exists in file so we get the info_hash
    if info_hash_start != 0 {
        let slice = &buf[info_hash_start..*pointer];
        let hash = get_hash(slice)?;
        dict.insert(String::from("info_hash"), BencodeTypes::InfoHash(hash));
    }

    // Place pointer at end of type (after "e")
    *pointer += 1;

    Ok(dict)
}

pub fn unwrap_string(string: BencodeTypes) -> Option<String> {
    if let BencodeTypes::String(s) = string {
        Some(s)
    } else {
        None
    }
}

pub fn unwrap_integer(int: BencodeTypes) -> Option<u32> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_string_and_advances_pointer() {
        let test_vec = b"11:HelloWorld!".to_vec();
        let mut pointer = 0;
        let result = decode_string(&mut pointer, &test_vec).unwrap();

        assert_eq!(String::from("HelloWorld!"), result);
        assert_eq!(pointer, 14);
    }

    #[test]
    fn decodes_int_and_advances_pointer() {
        let test_vec = b"i5657e".to_vec();
        let mut pointer = 0;
        let result = decode_int(&mut pointer, &test_vec).unwrap();

        assert_eq!(5657 as u32, result);
        assert_eq!(pointer, 6);
    }

    #[test]
    fn decodes_list_and_advances_pointer() {
        let test_vec =
            b"l11:HelloWorld!i5657el11:HelloWorld!i5657eed3:bar4:spam3:fooi42eee".to_vec();
        let mut pointer = 0;
        let result = decode_list(&mut pointer, &test_vec).unwrap();

        let string = String::from("HelloWorld!");
        let bar = String::from("bar");
        let spam = String::from("spam");
        let foo = String::from("foo");
        let int: u32 = 5657;

        let dict: HashMap<String, BencodeTypes> = HashMap::from([
            (bar.clone(), BencodeTypes::String(spam.clone())),
            (foo.clone(), BencodeTypes::Integer(42)),
        ]);

        assert_eq!(
            vec![
                BencodeTypes::String(string.clone()),
                BencodeTypes::Integer(int.clone()),
                BencodeTypes::List(vec![
                    BencodeTypes::String(string.clone()),
                    BencodeTypes::Integer(int.clone()),
                ]),
                BencodeTypes::Dict(dict),
            ],
            result,
        );
        assert_eq!(pointer, 66);
    }

    #[test]
    fn decodes_dict_and_advances_pointer() {
        let test_vec = b"d11:HelloWorld!i42e4:listll4:testel4:testeee".to_vec();
        let mut pointer = 0;
        let result = decode_dict(&mut pointer, &test_vec).unwrap();

        let string = String::from("HelloWorld!");
        let test = String::from("test");
        let list = String::from("list");
        let int: u32 = 42;

        let dict: HashMap<String, BencodeTypes> = HashMap::from([
            (string.clone(), BencodeTypes::Integer(int)),
            (
                list.clone(),
                BencodeTypes::List(vec![
                    BencodeTypes::List(vec![BencodeTypes::String(test.clone())]),
                    BencodeTypes::List(vec![BencodeTypes::String(test.clone())]),
                ]),
            ),
        ]);

        assert_eq!(dict, result);
        assert_eq!(pointer, 44);
    }
}
