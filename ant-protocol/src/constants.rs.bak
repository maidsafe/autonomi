// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use std::num::NonZeroUsize;

/// What is the largest packet to send over the network.
pub const MAX_PACKET_SIZE: usize = 1024 * 1024 * 5;

/// The maximum number of peers to return in a `GetClosestPeers` response.
/// This is the group size used in safe network protocol to be responsible for
/// an item in the network.
/// The peer should be present among the CLOSE_GROUP_SIZE if we're fetching the close_group(peer)
/// The size has been set to 5 for improved performance.
pub const CLOSE_GROUP_SIZE: usize = 5;

/// The protocol ID for the Kademlia stream
pub const KAD_STREAM_PROTOCOL_ID: &str = "/autonomi/kad/1.0.0";

/// The maximum size of a record
pub const MAX_RECORD_SIZE: usize = 1024 * 1024 * 4;

/// The replication factor we use on the network
/// Libp2p queries all depend on this, for quorum and others
/// Is defined as CLOSE_GROUP_SIZE + 2
pub const REPLICATION_FACTOR: NonZeroUsize =
    NonZeroUsize::new(7).expect("REPLICATION_FACTOR must be 7");
