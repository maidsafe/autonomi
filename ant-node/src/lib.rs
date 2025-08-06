// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! Implementation of the Node in SAFE Network.

// For quick_error
#![recursion_limit = "256"]
#![doc(
    html_logo_url = "https://github.com/maidsafe/QA/raw/master/Images/maidsafe_logo.png",
    html_favicon_url = "https://maidsafe.net/img/favicon.ico",
    test(attr(deny(warnings)))
)]
// Additional warnings and allows specific to this crate
// missing_docs and unwrap_used are already covered by workspace lints
#![warn(unreachable_pub, unused_results)]
#![allow(clippy::large_enum_variant)]
#![allow(clippy::result_large_err)]
// Allow expect and panic usage - to be refactored
#![allow(clippy::expect_used)]
#![allow(clippy::panic)]

#[macro_use]
extern crate tracing;

mod error;
mod event;
mod log_markers;
#[cfg(feature = "open-metrics")]
mod metrics;
mod networking;
mod node;
mod put_validation;
#[cfg(feature = "extension-module")]
mod python;
mod quote;
mod replication;
#[allow(missing_docs)]
pub mod spawn;
#[allow(missing_docs)]
pub mod utils;

pub use self::{
    error::{Error, PutValidationError},
    event::{NodeEvent, NodeEventsChannel, NodeEventsReceiver},
    log_markers::Marker,
    networking::ReachabilityStatus,
    networking::sort_peers_by_key,
    node::{NodeBuilder, PERIODIC_REPLICATION_INTERVAL_MAX_S},
};

use crate::error::Result;

use crate::networking::Network;
pub use crate::networking::SwarmLocalState;
use ant_evm::RewardsAddress;
use ant_protocol::{NetworkAddress, get_port_from_multiaddr};
use libp2p::{Multiaddr, PeerId};
use std::{
    collections::{BTreeMap, HashSet},
    path::PathBuf,
};
use tokio::sync::watch;

/// Once a node is started and running, the user obtains
/// a `NodeRunning` object which can be used to interact with it.
#[derive(Clone)]
pub struct RunningNode {
    shutdown_sender: watch::Sender<bool>,
    network: Network,
    node_events_channel: NodeEventsChannel,
    root_dir_path: PathBuf,
    rewards_address: RewardsAddress,
}

impl RunningNode {
    /// Returns this node's `PeerId`
    pub fn peer_id(&self) -> PeerId {
        self.network.peer_id()
    }

    /// Returns the root directory path for the node.
    ///
    /// This will either be a value defined by the user, or a default location, plus the peer ID
    /// appended. The default location is platform specific:
    ///  - Linux: $HOME/.local/share/autonomi/node/<peer-id>
    ///  - macOS: $HOME/Library/Application Support/autonomi/node/<peer-id>
    ///  - Windows: C:\Users\<username>\AppData\Roaming\autonomi\node\<peer-id>
    #[expect(rustdoc::invalid_html_tags)]
    pub fn root_dir_path(&self) -> PathBuf {
        self.root_dir_path.clone()
    }

    /// Returns a `SwarmLocalState` with some information obtained from swarm's local state.
    pub async fn get_swarm_local_state(&self) -> Result<SwarmLocalState> {
        let state = self.network.get_swarm_local_state().await?;
        Ok(state)
    }

    /// Return the node's listening addresses.
    pub async fn get_listen_addrs(&self) -> Result<Vec<Multiaddr>> {
        let listeners = self.network.get_swarm_local_state().await?.listeners;
        Ok(listeners)
    }

    /// Return the node's listening addresses with the peer id appended.
    pub async fn get_listen_addrs_with_peer_id(&self) -> Result<Vec<Multiaddr>> {
        let listeners = self.get_listen_addrs().await?;

        let multi_addrs: Vec<Multiaddr> = listeners
            .into_iter()
            .filter_map(|listen_addr| listen_addr.with_p2p(self.peer_id()).ok())
            .collect();

        Ok(multi_addrs)
    }

    /// Return the node's listening port
    pub async fn get_node_listening_port(&self) -> Result<u16> {
        let listen_addrs = self.network.get_swarm_local_state().await?.listeners;
        for addr in listen_addrs {
            if let Some(port) = get_port_from_multiaddr(&addr) {
                return Ok(port);
            }
        }
        Err(Error::FailedToGetNodePort)
    }

    /// Returns the node events channel where to subscribe to receive `NodeEvent`s
    pub fn node_events_channel(&self) -> &NodeEventsChannel {
        &self.node_events_channel
    }

    /// Returns the list of all the RecordKeys held by the node
    pub async fn get_all_record_addresses(&self) -> Result<HashSet<NetworkAddress>> {
        #[allow(clippy::mutable_key_type)] // for Bytes in NetworkAddress
        let addresses: HashSet<_> = self
            .network
            .get_all_local_record_addresses()
            .await?
            .keys()
            .cloned()
            .collect();
        Ok(addresses)
    }

    /// Returns a map where each key is the ilog2 distance of that Kbucket and each value is a vector of peers in that
    /// bucket.
    pub async fn get_kbuckets(&self) -> Result<BTreeMap<u32, Vec<PeerId>>> {
        let kbuckets = self.network.get_kbuckets().await?;
        Ok(kbuckets)
    }

    /// Returns the node's reward address
    pub fn reward_address(&self) -> &RewardsAddress {
        &self.rewards_address
    }

    /// Shutdown the network driver loop and the node (NetworkEvents) loop.
    pub fn shutdown(self) {
        // Send the shutdown signal to the swarm driver and node loop
        let _ = self.shutdown_sender.send(true);
    }
}
