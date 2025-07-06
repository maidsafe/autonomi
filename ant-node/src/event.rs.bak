// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::error::{Error, Result};

use ant_evm::AttoTokens;
use ant_protocol::{storage::ChunkAddress, NetworkAddress};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

const NODE_EVENT_CHANNEL_SIZE: usize = 500;

/// Channel where users of the public API can listen to events broadcasted by the node.
#[derive(Clone)]
pub struct NodeEventsChannel(broadcast::Sender<NodeEvent>);

/// Type of channel receiver where events are broadcasted to by the node.
pub type NodeEventsReceiver = broadcast::Receiver<NodeEvent>;

impl Default for NodeEventsChannel {
    fn default() -> Self {
        Self(broadcast::channel(NODE_EVENT_CHANNEL_SIZE).0)
    }
}

impl NodeEventsChannel {
    /// Returns a new receiver to listen to the channel.
    /// Multiple receivers can be actively listening.
    pub fn subscribe(&self) -> broadcast::Receiver<NodeEvent> {
        self.0.subscribe()
    }

    // Broadcast a new event, meant to be a helper only used by the ant-node's internals.
    pub(crate) fn broadcast(&self, event: NodeEvent) {
        let event_string = format!("{event:?}");
        if let Err(err) = self.0.send(event) {
            debug!(
                "Error occurred when trying to broadcast a node event ({event_string:?}): {err}"
            );
        }
    }

    /// Returns the number of active receivers
    pub fn receiver_count(&self) -> usize {
        self.0.receiver_count()
    }
}

/// Type of events broadcasted by the node to the public API.
#[derive(Clone, Serialize, custom_debug::Debug, Deserialize)]
pub enum NodeEvent {
    /// The node has been connected to the network
    ConnectedToNetwork,
    /// A Chunk has been stored in local storage
    ChunkStored(ChunkAddress),
    /// A new reward was received
    RewardReceived(AttoTokens, NetworkAddress),
    /// One of the sub event channel closed and unrecoverable.
    ChannelClosed,
    /// Terminates the node
    TerminateNode(String),
}

impl NodeEvent {
    /// Convert NodeEvent to bytes
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        rmp_serde::to_vec(&self).map_err(|_| Error::NodeEventParsingFailed)
    }

    /// Get NodeEvent from bytes
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        rmp_serde::from_slice(bytes).map_err(|_| Error::NodeEventParsingFailed)
    }
}
