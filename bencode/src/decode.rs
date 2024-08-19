use core::str;
use std::{collections::HashMap, net::Ipv4Addr, process::exit};

use sha1::{Digest, Sha1};

use crate::errors::ParseError;

const INTEGER_START: u8 = 0x69; // 'i'
const STRING_DELIM: u8 = 0x3A; // ':'
const DICTIONARY_START: u8 = 0x64; // 'd'
const LIST_START: u8 = 0x6C; // 'l'
const END_OF_TYPE: u8 = 0x65; // 'e'

#[derive(Debug, PartialEq)]
pub enum BencodeTypes {
    String(String),
    Integer(u32),
    List(Vec<BencodeTypes>),
    Dict(HashMap<String, BencodeTypes>),
    InfoHash([u8; 20]),
    Pieces(Vec<[u8; 20]>),
    PeersCompact(Vec<(Ipv4Addr, u16)>),
}

fn parse_to_utf8(slice: &[u8]) -> Result<String, ParseError> {
    let string_slice = str::from_utf8(slice)?;
    Ok(string_slice.to_string())
}

fn parse_to_usize(slice: &[u8]) -> Result<usize, ParseError> {
    let string = parse_to_utf8(slice)?;
    Ok(string.parse()?)
}

pub fn decode_string(pointer: &mut usize, buf: &Vec<u8>) -> Result<String, ParseError> {
    let mut string_len_bytes = Vec::new();

    while buf[*pointer] != STRING_DELIM {
        string_len_bytes.push(buf[*pointer]);
        *pointer += 1;
    }
    *pointer += 1;

    if string_len_bytes.len() == 1 && string_len_bytes[0] == 48 {
        return Ok(String::from(""));
    }

    let string_len = parse_to_usize(&string_len_bytes)? + *pointer;
    let slice: &[u8] = &buf[*pointer..string_len];

    *pointer = string_len;

    Ok(parse_to_utf8(slice)?)
}

pub fn decode_int(pointer: &mut usize, buf: &Vec<u8>) -> Result<u32, ParseError> {
    let mut int_len_bytes = Vec::new();

    *pointer += 1;

    while buf[*pointer] != END_OF_TYPE {
        int_len_bytes.push(buf[*pointer]);
        *pointer += 1;
    }

    *pointer += 1;

    Ok(parse_to_usize(&int_len_bytes)? as u32)
}

pub fn decode_list(pointer: &mut usize, buf: &Vec<u8>) -> Result<Vec<BencodeTypes>, ParseError> {
    let mut list: Vec<BencodeTypes> = Vec::new();

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

    *pointer += 1;

    Ok(list)
}

fn get_hash(slice: &[u8]) -> Result<[u8; 20], ParseError> {
    let mut hasher = Sha1::new();
    hasher.update(slice);
    Ok(hasher.finalize().into())
}

pub fn decode_pieces(pointer: &mut usize, buf: &Vec<u8>) -> Result<Vec<[u8; 20]>, ParseError> {
    let mut pieces_len_bytes = Vec::new();

    while buf[*pointer] != STRING_DELIM {
        pieces_len_bytes.push(buf[*pointer]);
        *pointer += 1;
    }
    *pointer += 1;

    let pieces_len = parse_to_usize(&pieces_len_bytes)? + *pointer;

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

    *pointer = pieces_len;

    Ok(pieces_vec)
}

fn decode_peers(pointer: &mut usize, buf: &Vec<u8>) -> Result<BencodeTypes, ParseError> {
    if buf[*pointer] == LIST_START {
        let decoded = decode_list(pointer, buf)?;
        return Ok(BencodeTypes::List(decoded));
    }

    let mut peers_len_bytes = Vec::new();

    while buf[*pointer] != STRING_DELIM {
        peers_len_bytes.push(buf[*pointer]);
        *pointer += 1;
    }
    *pointer += 1;

    let peers_len = parse_to_usize(&peers_len_bytes)? + *pointer;

    let peers_vec: Vec<(Ipv4Addr, u16)> = buf[*pointer..peers_len]
        .chunks_exact(6)
        .map(|item| {
            let ip = Ipv4Addr::new(item[0], item[1], item[2], item[3]);
            let port = u16::from_be_bytes([item[4], item[5]]);
            (ip, port)
        })
        .collect();

    *pointer = peers_len;

    Ok(BencodeTypes::PeersCompact(peers_vec))
}

pub fn decode_dict(
    pointer: &mut usize,
    buf: &Vec<u8>,
) -> Result<HashMap<String, BencodeTypes>, ParseError> {
    if buf.len() == 0 {
        return Err(ParseError::BadFile);
    }

    let mut dict: HashMap<String, BencodeTypes> = HashMap::new();

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

    *pointer += 1;

    Ok(dict)
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
