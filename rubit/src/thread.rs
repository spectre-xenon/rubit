use std::{
    collections::{HashSet, VecDeque},
    fs::File,
    net::SocketAddr,
    sync::{Arc, Mutex},
    thread::{self, JoinHandle},
};

use bencode::TorrentFile;

use crate::PeerConnManager;

pub struct PeerManager {
    pub peers: Arc<Mutex<HashSet<SocketAddr>>>,
}

impl PeerManager {
    pub fn new() -> Self {
        Self {
            peers: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    pub fn try_add(
        &self,
        global_queue: Arc<Mutex<VecDeque<usize>>>,
        socket_addr: SocketAddr,
        torrent_file: Arc<TorrentFile>,
        peer_id: [u8; 20],
        file: Arc<Mutex<File>>,
    ) -> Option<JoinHandle<()>> {
        let mut set = self.peers.lock().unwrap();

        if set.insert(socket_addr) {
            let peers_clone = Arc::clone(&self.peers);
            Some(thread::spawn(move || {
                let mut peer_manager = PeerConnManager::new();

                match peer_manager.handle_peer(
                    global_queue,
                    socket_addr,
                    torrent_file,
                    peer_id,
                    file,
                ) {
                    Err(_) => {
                        let mut set = peers_clone.lock().unwrap();
                        set.remove(&socket_addr);
                    }
                    _ => (),
                };
            }))
        } else {
            None
        }
    }
}
