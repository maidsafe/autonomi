// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use libp2p::PeerId;
use serde::{Deserialize, Serialize};
use std::fmt::{self, Display};

/// A unique identifier for a node in the network,
/// by which we can know their location in the xor space.
#[derive(Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct NodeId(Vec<u8>);

impl NodeId {
    /// Returns a `NodeId` representation of the `PeerId` by encapsulating its bytes.
    pub fn from(peer_id: PeerId) -> Self {
        Self(peer_id.to_bytes())
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
