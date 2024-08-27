use std::{
    collections::{HashSet, VecDeque},
    fs::{self, File},
    io::{Read, Seek, SeekFrom},
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    process::exit,
    sync::{Arc, Mutex},
    thread,
    time::{self, Duration},
};

use bencode::TorrentFile;
use indicatif::{ProgressBar, ProgressStyle};
use rand::{distributions::Alphanumeric, thread_rng, Rng};
use rubit::{AnnounceConfig, FailureResponse, PeerManager, Responses, Tracker};
use sha1::{Digest, Sha1};

use rand::seq::SliceRandom;

use url::Url;

fn get_random_id() -> String {
    let mut peer_id = String::from("RB01-");
    let random_15: String = thread_rng()
        .sample_iter(&Alphanumeric)
        .take(15)
        .map(char::from)
        .collect();
    peer_id.push_str(&random_15);
    peer_id
}

fn main() {
    let file_buf = fs::read("test.torrent").unwrap();
    let torrent_file = TorrentFile::from(file_buf);

    let piece_num = torrent_file.info.pieces.len();

    let file = Arc::new(Mutex::new(
        File::options()
            .write(true)
            .read(true)
            .create(true)
            .open(&torrent_file.info.name)
            .unwrap(),
    ));

    let completed = check_download_percent(
        file.clone(),
        &torrent_file.info.pieces,
        torrent_file.info.length,
        torrent_file.info.piece_length,
    );

    let progress_bar = ProgressBar::new(100);

    progress_bar.set_style(
        ProgressStyle::with_template("[{elapsed_precise}] {wide_bar:.cyan/blue} {pos}% {msg:>7}")
            .unwrap()
            .progress_chars("##-"),
    );

    let last_value = ((completed.len() as f64 / piece_num as f64) * 100f64).floor() as u64;

    let pieces_queue = Vec::from((0..torrent_file.info.pieces.len()).collect::<Vec<usize>>());

    let mut cleaned_vec = retain_not_downloaded_pieces(completed, pieces_queue);

    if cleaned_vec.is_empty() {
        println!("File is already completed! Exiting...");
        exit(0)
    }

    println!("Downloading...");
    progress_bar.inc(last_value);

    cleaned_vec.shuffle(&mut thread_rng());

    let global_queue = Arc::new(Mutex::new(VecDeque::from(cleaned_vec)));

    let peer_id = get_random_id();

    let peer_manager = PeerManager::new();

    let announce_list = match torrent_file.announce_list.clone() {
        Some(a) => a,
        None => vec![vec![String::from("i guess no announce list")]],
    };

    let tracker_list = get_tracker_list(torrent_file.announce.clone(), announce_list);

    let mut announce_instant = time::Instant::now();
    let mut duration = Duration::from_millis(1);

    let poll_duration = Duration::from_millis(250);
    let mut poll_instant = time::Instant::now();

    let shared_torrent_file = Arc::new(torrent_file);
    let mut handles = Vec::new();

    loop {
        if poll_instant.elapsed() > poll_duration {
            let queue_len = global_queue.lock().unwrap().len();
            let peers_len = peer_manager.peers.lock().unwrap().len();
            print!("\r\033[K");

            let value = (100f64 - ((queue_len as f64 / piece_num as f64) * 100f64)).floor() as u64;
            progress_bar.set_position(value);
            progress_bar.set_message(format!("Peers: {}", peers_len));

            poll_instant = time::Instant::now();
        }

        let set = peer_manager.peers.lock().unwrap();
        if set.len() > 30 {
            continue;
        }

        std::mem::drop(set);

        if announce_instant.elapsed() < duration {
            continue;
        }

        #[allow(unused)]
        let mut response: Responses = Responses::Failure(FailureResponse {
            failure_reason: String::from("got no response"),
        });

        loop {
            match tracker_list[thread_rng().gen_range(0..tracker_list.len())].announce(
                AnnounceConfig {
                    info_hash: shared_torrent_file.info_hash,
                    downloaded: 0,
                    left: shared_torrent_file.info.length,
                    uploaded: 0,
                    peer_id: peer_id.to_string(),
                    port: 6881,
                },
            ) {
                Ok(r) => {
                    response = r;
                    break;
                }
                Err(_) => continue,
            };
        }

        let result = match response {
            rubit::Responses::Done(d) => d,
            rubit::Responses::Failure(f) => {
                println!("failed with reason: {}", f.failure_reason);
                panic!()
            }
        };

        duration = match result.min_interval {
            Some(i) => i,
            None => result.interval,
        };
        announce_instant = time::Instant::now();

        for (octets, port) in result.peers {
            let socket_addr = SocketAddr::V4(SocketAddrV4::new(
                Ipv4Addr::new(octets.0, octets.1, octets.2, octets.3),
                port,
            ));
            let global_queue = Arc::clone(&global_queue);
            let torrent_file = Arc::clone(&shared_torrent_file);

            let handle = peer_manager.try_add(
                global_queue,
                socket_addr,
                torrent_file,
                peer_id.clone().as_bytes().try_into().unwrap(),
                file.clone(),
            );

            match handle {
                Some(h) => handles.push(h),
                None => (),
            }
        }

        let queue = global_queue.lock().unwrap();
        if queue.is_empty() {
            break;
        }
        std::mem::drop(queue);
        thread::sleep(Duration::from_millis(1));
    }

    for handle in handles {
        handle.join().unwrap();
    }
}

fn get_tracker_list(announce: String, announce_list: Vec<Vec<String>>) -> Vec<Tracker> {
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

fn check_download_percent(
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

fn retain_not_downloaded_pieces(completed: HashSet<usize>, mut buf: Vec<usize>) -> Vec<usize> {
    buf.retain(|e| !completed.contains(e));
    buf
}
