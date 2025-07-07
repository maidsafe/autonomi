// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! # ant-net
//!
//! Network abstraction layer for the Autonomi Network.
//!
//! This crate provides a clean abstraction over libp2p networking functionality,
//! designed to encapsulate complexity and provide consistent APIs across the Autonomi project.
//!
//! ## Architecture
//!
//! The abstraction is organized into several key components:
//!
//! - **Transport Layer**: Abstract transport configuration and connection management
//! - **Behavior System**: Composable network behaviors (Kademlia, Request/Response, etc.)
//! - **Event Processing**: Unified event handling with priority-based processing
//! - **Connection Management**: Abstract connection lifecycle and state tracking
//! - **Protocol Handling**: Generic protocol interfaces and message routing
//!
//! ## Example
//!
//! ```rust,no_run
//! use ant_net::{AntNet, AntNetBuilder};
//!
//! # #[tokio::main]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! // Build a network instance
//! let network = AntNetBuilder::new()
//!     .build()
//!     .await?;
//!
//! // Start the network
//! network.start().await?;
//! # Ok(())
//! # }
//! ```

#![warn(missing_docs)]
#![warn(clippy::unwrap_used)]

// Core modules
pub mod types;
pub mod transport;
pub mod behavior;
pub mod behavior_manager;
pub mod behaviors;
pub mod bridge;
pub mod connection;
pub mod event;
pub mod event_router;
pub mod error;
pub mod protocol;
pub mod driver;
pub mod builder;

// Re-export the main API
pub use builder::{AntNetBuilder, AntNetConfig};
pub use driver::AntNet;
pub use error::{AntNetError, Result};

// Re-export commonly used types to avoid dependency version conflicts
pub use types::{PeerId, Multiaddr, ConnectionId, NetworkAddress};
pub use event::NetworkEvent;
pub use transport::Transport;
pub use behavior::NetworkBehaviour;
pub use behavior_manager::{BehaviorController, BehaviorManager, BehaviorHealth};
pub use behaviors::{IdentifyBehaviorWrapper, IdentifyBehaviorConfig, DoNotDisturbBehaviorWrapper, DoNotDisturbConfig, RequestResponseBehaviorWrapper, RequestResponseConfig, KademliaBehaviorWrapper, KademliaBehaviorConfig};
pub use bridge::{LibP2pBehaviorBridge, NetworkBridge, BridgedSwarm};
pub use connection::{Connection, ConnectionManager};
pub use event_router::{EventRouter, EventSubscriber, RoutableEvent};
pub use protocol::{RequestResponse, ProtocolHandler};

// Re-export ant-kad types that we wrap
pub use ant_kad::{Record, RecordKey, KBucketDistance, KBucketKey};

// Re-export bytes for convenience
pub use bytes::Bytes;