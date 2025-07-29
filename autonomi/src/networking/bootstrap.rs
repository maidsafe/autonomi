use crate::networking::PeerId;
use std::collections::HashSet;
use tokio::sync::oneshot;
use tracing::error;

pub(crate) const MAX_REQUIRED_PEERS: u32 = 25;
pub(crate) const MAX_BOOTSTRAP_DURATION_SECS: u64 = 10;

pub(crate) struct BootstrapManager {
    connected_peers: HashSet<PeerId>,
    observers: Vec<(oneshot::Sender<u32>, u32)>,
}

impl BootstrapManager {
    pub fn new() -> Self {
        Self {
            connected_peers: Default::default(),
            observers: Default::default(),
        }
    }

    pub fn register_observer(&mut self, observer: (oneshot::Sender<u32>, u32)) {
        self.observers.push(observer);
    }

    pub fn add_connected_peer(&mut self, peer_id: PeerId) {
        self.connected_peers.insert(peer_id);

        // Process observers and remove those whose conditions are met or that fail to send
        let mut i = 0;
        while i < self.observers.len() {
            let required_peers = self.observers[i].1;
            if self.connected_peers.len() >= required_peers as usize {
                // Remove the observer and attempt to notify it
                let (observer_callback, _) = self.observers.swap_remove(i);
                if let Err(err) = observer_callback.send(self.connected_peers.len() as u32) {
                    error!("Failed to send bootstrap result to observer: {err:?}");
                }
                // Don't increment i since we removed an element
            } else {
                i += 1;
            }
        }
    }
}
