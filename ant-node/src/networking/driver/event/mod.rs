// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

mod identify;
mod kad;
mod request_response;
mod swarm;

pub(crate) use identify::DIAL_BACK_DELAY;

use crate::networking::NetworkEvent;
use crate::networking::driver::behaviour::upnp;
use crate::networking::{Addresses, driver::SwarmDriver, error::Result};
use ant_protocol::messages::ConnectionInfo;
use custom_debug::Debug as CustomDebug;
use libp2p::kad::K_VALUE;
use libp2p::{PeerId, request_response::ResponseChannel as PeerResponseChannel};

use ant_protocol::CLOSE_GROUP_SIZE;
use ant_protocol::{
    NetworkAddress,
    messages::{Request, Response},
};
#[cfg(feature = "open-metrics")]
use std::collections::HashSet;
use tokio::sync::oneshot;

#[derive(Debug, Clone)]
pub(crate) struct KBucketStatus {
    pub(crate) total_buckets: usize,
    pub(crate) total_peers: usize,
    pub(crate) peers_in_non_full_buckets: usize,
    pub(crate) num_of_full_buckets: usize,
    pub(crate) kbucket_table_stats: Vec<(usize, usize, u32)>,
    pub(crate) estimated_network_size: usize,
}

impl KBucketStatus {
    pub(crate) fn log(&self) {
        info!(
            "kBucketTable has {:?} kbuckets {:?} peers, {:?}, estimated network size: {:?}",
            self.total_buckets,
            self.total_peers,
            self.kbucket_table_stats,
            self.estimated_network_size
        );
        #[cfg(feature = "loud")]
        println!("Estimated network size: {:?}", self.estimated_network_size);
    }
}

/// NodeEvent enum
#[derive(CustomDebug)]
pub(crate) enum NodeEvent {
    Upnp(upnp::behaviour::Event),
    MsgReceived(libp2p::request_response::Event<Request, Response>),
    Kademlia(libp2p::kad::Event),
    Identify(Box<libp2p::identify::Event>),
    DoNotDisturb(super::behaviour::do_not_disturb::DoNotDisturbEvent),
    Void(void::Void),
}

impl From<upnp::behaviour::Event> for NodeEvent {
    fn from(event: upnp::behaviour::Event) -> Self {
        NodeEvent::Upnp(event)
    }
}

impl From<libp2p::request_response::Event<Request, Response>> for NodeEvent {
    fn from(event: libp2p::request_response::Event<Request, Response>) -> Self {
        NodeEvent::MsgReceived(event)
    }
}

impl From<libp2p::kad::Event> for NodeEvent {
    fn from(event: libp2p::kad::Event) -> Self {
        NodeEvent::Kademlia(event)
    }
}

impl From<libp2p::identify::Event> for NodeEvent {
    fn from(event: libp2p::identify::Event) -> Self {
        NodeEvent::Identify(Box::new(event))
    }
}

impl From<super::behaviour::do_not_disturb::DoNotDisturbEvent> for NodeEvent {
    fn from(event: super::behaviour::do_not_disturb::DoNotDisturbEvent) -> Self {
        NodeEvent::DoNotDisturb(event)
    }
}

impl From<void::Void> for NodeEvent {
    fn from(event: void::Void) -> Self {
        NodeEvent::Void(event)
    }
}

#[allow(clippy::type_complexity)]
#[derive(CustomDebug)]
/// Channel to send the `Response` through.
pub(crate) enum MsgResponder {
    /// Respond to a request from `self` through a simple one-shot channel.
    FromSelf(Option<oneshot::Sender<Result<(Response, Option<ConnectionInfo>)>>>),
    /// Respond to a request from a peer in the network.
    FromPeer(PeerResponseChannel<Response>),
}

impl SwarmDriver {
    /// Check for changes in our close group
    #[cfg(feature = "open-metrics")]
    pub(crate) fn check_for_change_in_our_close_group(&mut self) {
        // this includes self

        let closest_k_peers = self.get_closest_k_local_peers_to_self();

        let new_closest_peers: Vec<PeerId> = closest_k_peers
            .into_iter()
            .map(|(peer_id, _)| peer_id)
            .take(CLOSE_GROUP_SIZE)
            .collect();

        let old = self.close_group.iter().cloned().collect::<HashSet<_>>();
        let new_members: Vec<_> = new_closest_peers
            .iter()
            .filter(|p| !old.contains(p))
            .collect();
        if !new_members.is_empty() {
            debug!("The close group has been updated. The new members are {new_members:?}");
            debug!("New close group: {new_closest_peers:?}");
            self.close_group = new_closest_peers.clone();
            self.record_change_in_close_group(new_closest_peers);
        }
    }

    /// Update state on addition of a peer to the routing table.
    pub(crate) fn update_on_peer_addition(&mut self, added_peer: PeerId, addresses: Addresses) {
        let kbucket_status = self.get_kbuckets_status();
        self.update_on_kbucket_status(&kbucket_status);

        let distance =
            NetworkAddress::from(self.self_peer_id).distance(&NetworkAddress::from(added_peer));
        // ELK logging. Do not update without proper testing.
        info!(
            "Node {:?} added new peer into routing table: {added_peer:?}. It has a {:?} distance to us.",
            self.self_peer_id,
            distance.ilog2()
        );

        #[cfg(feature = "loud")]
        println!(
            "New peer added to routing table: {added_peer:?}, now we have #{} connected peers",
            self.peers_in_rt
        );

        kbucket_status.log();

        if let Some(bootstrap_cache) = &self.bootstrap_cache {
            let bootstrap_cache = bootstrap_cache.clone();
            #[allow(clippy::let_underscore_future)]
            let _ = tokio::spawn(async move {
                for addr in addresses.0.into_iter() {
                    bootstrap_cache.add_addr(addr).await
                }
            });
        }

        self.send_event(NetworkEvent::PeerAdded(added_peer, self.peers_in_rt));

        #[cfg(feature = "open-metrics")]
        if self.metrics_recorder.is_some() {
            self.check_for_change_in_our_close_group();
        }
    }

    /// Update state on removal of a peer from the routing table.
    pub(crate) fn update_on_peer_removal(&mut self, removed_peer: PeerId) {
        let kbucket_status = self.get_kbuckets_status();
        self.update_on_kbucket_status(&kbucket_status);

        // ensure we disconnect bad peer
        // err result just means no connections were open
        let _result = self.swarm.disconnect_peer_id(removed_peer);

        let distance =
            NetworkAddress::from(self.self_peer_id).distance(&NetworkAddress::from(removed_peer));
        // ELK logging. Do not update without proper testing.
        info!(
            "Peer removed from routing table: {removed_peer:?}. We now have #{} connected peers. It has a {:?} distance to us.",
            self.peers_in_rt,
            distance.ilog2()
        );

        self.send_event(NetworkEvent::PeerRemoved(removed_peer, self.peers_in_rt));

        if let Some(bootstrap_cache) = &self.bootstrap_cache {
            let removed_peer_clone = removed_peer;
            let bootstrap_cache = bootstrap_cache.clone();
            #[allow(clippy::let_underscore_future)]
            let _ = tokio::spawn(async move {
                bootstrap_cache.remove_peer(&removed_peer_clone).await;
            });
        }

        kbucket_status.log();

        #[cfg(feature = "open-metrics")]
        if self.metrics_recorder.is_some() {
            self.check_for_change_in_our_close_group();
        }
    }

    /// Get the status of the kbucket table.
    pub(crate) fn get_kbuckets_status(&mut self) -> KBucketStatus {
        let mut kbucket_table_stats = vec![];
        let mut index = 0;
        let mut total_peers = 0;
        let mut peers_in_non_full_buckets = 0;
        let mut num_of_full_buckets = 0;

        for kbucket in self.swarm.behaviour_mut().kademlia.kbuckets() {
            let range = kbucket.range();
            let num_entires = kbucket.num_entries();

            if num_entires >= K_VALUE.get() {
                num_of_full_buckets += 1;
            } else {
                peers_in_non_full_buckets += num_entires;
            }

            total_peers += num_entires;
            if let Some(distance) = range.0.ilog2() {
                kbucket_table_stats.push((index, num_entires, distance));
            } else {
                // This shall never happen.
                error!("bucket #{index:?} is ourself ???!!!");
            }
            index += 1;
        }

        let estimated_network_size =
            Self::estimate_network_size(peers_in_non_full_buckets, num_of_full_buckets);

        KBucketStatus {
            total_buckets: index,
            total_peers,
            peers_in_non_full_buckets,
            num_of_full_buckets,
            kbucket_table_stats,
            estimated_network_size,
        }
    }

    /// Update SwarmDriver field & also record metrics based on the newly calculated kbucket status.
    pub(crate) fn update_on_kbucket_status(&mut self, status: &KBucketStatus) {
        self.peers_in_rt = status.total_peers;
        #[cfg(feature = "open-metrics")]
        if let Some(metrics_recorder) = &self.metrics_recorder {
            let _ = metrics_recorder
                .peers_in_routing_table
                .set(status.total_peers as i64);

            let estimated_network_size = Self::estimate_network_size(
                status.peers_in_non_full_buckets,
                status.num_of_full_buckets,
            );
            let _ = metrics_recorder
                .estimated_network_size
                .set(estimated_network_size as i64);
        }
    }

    /// Estimate the number of nodes in the network
    pub(crate) fn estimate_network_size(
        peers_in_non_full_buckets: usize,
        num_of_full_buckets: usize,
    ) -> usize {
        (peers_in_non_full_buckets + 1) * (2_usize.pow(num_of_full_buckets as u32))
    }
}
