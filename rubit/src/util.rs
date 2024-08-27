use std::{
    collections::HashSet,
    fs::File,
    io::{Read, Seek, SeekFrom},
    sync::{Arc, Mutex},
};

use rand::{distributions::Alphanumeric, thread_rng, Rng};
use sha1::{Digest, Sha1};
use url::Url;

use crate::Tracker;

pub fn get_random_id() -> String {
    let mut peer_id = String::from("RB01-");
    let random_15: String = thread_rng()
        .sample_iter(&Alphanumeric)
        .take(15)
        .map(char::from)
        .collect();
    peer_id.push_str(&random_15);
    peer_id
}

pub fn get_tracker_list(announce: String, announce_list: Vec<Vec<String>>) -> Vec<Tracker> {
    let mut flattened_list: Vec<&String> = announce_list.iter().flatten().collect();
    flattened_list.push(&announce);

    let mut vec = Vec::new();

    for url in flattened_list {
        let parsed = match Url::parse(&url) {
            Ok(u) => u,
            Err(_) => continue,
        };
        let tracker = match Tracker::new(parsed) {
            Ok(t) => t,
            Err(_) => continue,
        };

        vec.push(tracker);
    }
    vec
}

pub fn check_download_percent(
    file: Arc<Mutex<File>>,
    pieces: &Vec<[u8; 20]>,
    total_length: u64,
    piece_len: u64,
) -> HashSet<usize> {
    println!("File already exists, checking downloaded hashes...");

    let mut file = file.lock().unwrap();
    if file.seek(SeekFrom::End(0)).unwrap() == 0 {
        return HashSet::new();
    }

    let mut completed = HashSet::new();
    let mut cursor = 0;

    for i in 0..pieces.len() {
        let mut buf = Vec::new();
        if i == pieces.len() - 1 {
            buf.resize((total_length % piece_len) as usize, 0);
        } else {
            buf.resize(piece_len as usize, 0);
        }
        let mut hasher = Sha1::new();

        file.seek(SeekFrom::Start(cursor)).unwrap();
        file.read(&mut buf).unwrap();

        hasher.update(&buf);
        let hash: [u8; 20] = hasher.finalize().into();

        if hash == pieces[i] {
            completed.insert(i);
        }

        cursor += piece_len;
    }

    completed
}

pub fn retain_not_downloaded_pieces(completed: HashSet<usize>, mut buf: Vec<usize>) -> Vec<usize> {
    buf.retain(|e| !completed.contains(e));
    buf
}
