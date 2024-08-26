use std::{
    collections::VecDeque,
    fs::{self, File},
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    sync::{Arc, Mutex},
};

use bencode::TorrentFile;
use rand::{distributions::Alphanumeric, thread_rng, Rng};
use rubit::{AnnounceConfig, PeerManager, Tracker};

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
    println!("last piece size: {}", torrent_file.info.length / 16384);
    let file = Arc::new(Mutex::new(
        File::options()
            .write(true)
            .read(true)
            .create(true)
            .open(&torrent_file.info.name)
            .unwrap(),
    ));

    let peer_id = get_random_id();

    let url = Url::parse(&torrent_file.announce).unwrap();
    let main_tracker = Tracker::new(url).unwrap();
    let response = main_tracker
        .announce(AnnounceConfig {
            info_hash: torrent_file.info_hash,
            downloaded: 0,
            left: torrent_file.info.length,
            uploaded: 0,
            peer_id: peer_id.to_string(),
            port: 6881,
        })
        .unwrap();

    let result = match response {
        rubit::Responses::Done(d) => d,
        rubit::Responses::Failure(f) => {
            println!("failed with reason: {}", f.failure_reason);
            panic!()
        }
    };

    let mut vec = Vec::from((0..torrent_file.info.pieces.len()).collect::<Vec<usize>>());

    vec.shuffle(&mut thread_rng());

    let global_queue = Arc::new(Mutex::new(VecDeque::from(vec)));

    let peer_manager = PeerManager::new();

    let mut handles = Vec::new();
    let shared_torrent_file = Arc::new(torrent_file);
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

    for handle in handles {
        handle.join().unwrap();
    }
}
