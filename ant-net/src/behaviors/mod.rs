// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! Behavior implementations for ant-net.
//!
//! This module contains ant-net wrappers for various libp2p behaviors,
//! providing clean abstractions that integrate with the ant-net event system.

pub mod identify;
pub mod do_not_disturb;
pub mod request_response;
pub mod kademlia;

// Re-export behavior types
pub use identify::{IdentifyBehaviorWrapper, IdentifyBehaviorConfig, PeerInfo as IdentifyPeerInfo, IdentifyStats};
pub use do_not_disturb::{DoNotDisturbBehaviorWrapper, DoNotDisturbConfig, DoNotDisturbEntry, DoNotDisturbMessage, DoNotDisturbStats};
pub use request_response::{RequestResponseBehaviorWrapper, RequestResponseConfig, PendingRequest, RequestResponseStats};
pub use kademlia::{KademliaBehaviorWrapper, KademliaBehaviorConfig, QueryInfo, QueryType, KademliaStats};