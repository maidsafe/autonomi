// Copyright 2023 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

mod api;
mod error;
mod event;

pub use self::event::{NodeEvent, NodeEventsChannel, NodeEventsReceiver};

use self::error::Error;

use crate::{
    network::Network,
    protocol::node_transfers::Transfers,
    storage::{ChunkStorage, RegisterStorage},
};

use libp2p::{Multiaddr, PeerId};
use serde::{Deserialize, Serialize};
use std::fmt::{self, Display};
use xor_name::XorName;

/// `Node` represents a single node in the distributed network. It handles
/// network events, processes incoming requests, interacts with the data
/// storage, and broadcasts node-related events.
pub struct Node {
    network: Network,
    chunks: ChunkStorage,
    registers: RegisterStorage,
    transfers: Transfers,
    events_channel: NodeEventsChannel,
    /// Peers that are dialed at startup of node.
    initial_peers: Vec<(PeerId, Multiaddr)>,
}

/// A unique identifier for a node in the network,
/// by which we can know their location in the xor space.
#[derive(
    Copy, Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize,
)]
pub struct NodeId(XorName);

impl NodeId {
    /// Returns a `NodeId` representation of the `PeerId` by hashing its bytes.
    pub fn from(peer_id: PeerId) -> Self {
        Self(XorName::from_content(&peer_id.to_bytes()))
    }

    /// Returns this NodeId as bytes
    pub fn as_bytes(&self) -> Vec<u8> {
        self.0.to_vec()
    }
}

impl Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "NodeId({:?})", self.0)
    }
}
