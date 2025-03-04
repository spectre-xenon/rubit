use std::{
    collections::VecDeque,
    fs::{self, File},
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    process::exit,
    sync::{Arc, Mutex},
    time::{self, Duration},
};

use clap::Parser;
use indicatif::{ProgressBar, ProgressStyle};
use rand::thread_rng;
use rubit::{
    check_download_percent, get_random_id, get_tracker_list, retain_not_downloaded_pieces,
    AnnounceConfig, FailureResponse, PeerManager, Responses,
};

use rand::seq::SliceRandom;
use rubit_bencode::TorrentFile;

/// Simple Bittorrent client capable of downloading meta-info (.torrent) files,
/// Writen in Rust!
#[derive(Parser, Debug)]
#[command(version, about)]
struct Args {
    /// Path of the .torrent file to download
    #[arg(short = 't', long)]
    torrent_file: String,
    /// [Optional] Output file Path [default: the directory rubit was run in ]
    #[arg(short = 'o', long)]
    out: Option<String>,
    /// [Optional] The interval to re-announce on in Secs\n
    /// Some trackers return long intervals e.g. 30min
    /// You can set this option to something like 30s to get more peers
    #[arg(short = 'i', long)]
    interval: Option<u64>,
    /// [Optional] Print extra logs, needed for development and will omit the progress bar
    #[arg(short = 'V', long, action)]
    verbose: bool,
}

fn main() {
    let args = Args::parse();

    let file_buf = match fs::read(args.torrent_file) {
        Ok(f) => f,
        Err(e) => {
            println!("failed to read torrent file with Err: {}", e);
            exit(1)
        }
    };

    let torrent_file = TorrentFile::from(file_buf);

    let piece_num = torrent_file.info.pieces.len();

    let path_string = match args.out {
        Some(s) => &s.clone(),
        None => &torrent_file.info.name,
    };

    let file = Arc::new(Mutex::new(
        match File::options()
            .write(true)
            .read(true)
            .create(true)
            .open(path_string)
        {
            Ok(f) => f,
            Err(e) => {
                println!("failed to create file with Err: {}", e);
                exit(1)
            }
        },
    ));

    let completed = check_download_percent(
        file.clone(),
        &torrent_file.info.pieces,
        torrent_file.info.length,
        torrent_file.info.piece_length,
    );

    let progress_bar = ProgressBar::new(100);
    let poll_duration = Duration::from_millis(250);
    let mut poll_instant = time::Instant::now();

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
    let mut current_tracker_index = 0;

    let mut announce_instant = time::Instant::now();
    let mut duration = Duration::from_millis(1);

    let shared_torrent_file = Arc::new(torrent_file);
    let mut handles = Vec::new();

    loop {
        if poll_instant.elapsed() > poll_duration && !args.verbose {
            let queue_len = global_queue.lock().unwrap().len();
            let peers_len = peer_manager.peers.lock().unwrap().len();
            print!("\r\033[K");

            let value = (100f64 - ((queue_len as f64 / piece_num as f64) * 100f64)).floor() as u64;
            progress_bar.set_position(value);
            progress_bar.set_message(format!("Peers: {}", peers_len));

            poll_instant = time::Instant::now();
        }

        let queue = global_queue.lock().unwrap();
        let peers = peer_manager.peers.lock().unwrap();

        if queue.is_empty() && peers.is_empty() {
            println!("Download finished");
            break;
        }

        if peers.len() > 300 && !peers.is_empty() {
            continue;
        }

        if announce_instant.elapsed() < duration && !peers.is_empty() {
            continue;
        }

        std::mem::drop(queue);
        std::mem::drop(peers);

        #[allow(unused)]
        let mut response: Responses = Responses::Failure(FailureResponse {
            failure_reason: String::from("got no response"),
        });

        loop {
            match tracker_list[current_tracker_index].announce(AnnounceConfig {
                info_hash: shared_torrent_file.info_hash,
                downloaded: 0,
                left: shared_torrent_file.info.length,
                uploaded: 0,
                peer_id: peer_id.to_string(),
                port: 6881,
            }) {
                Ok(r) => {
                    response = r;
                    break;
                }
                Err(_) => {
                    current_tracker_index += 1;
                    continue;
                }
            };
        }

        let result = match response {
            rubit::Responses::Done(d) => d,
            rubit::Responses::Failure(f) => {
                println!("failed with reason: {}", f.failure_reason);
                panic!()
            }
        };

        if let Some(d) = args.interval {
            duration = Duration::from_secs(d)
        } else {
            duration = match result.min_interval {
                Some(i) => i,
                None => result.interval,
            };
        }

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
                args.verbose,
            );

            match handle {
                Some(h) => handles.push(h),
                None => (),
            }
        }
    }

    for handle in handles {
        handle.join().unwrap();
    }
}
