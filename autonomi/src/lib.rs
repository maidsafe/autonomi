// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! Connect to and build on the Autonomi network.
//!
//! # Example
//!
//! ```no_run
//! use autonomi::{Bytes, Client, Wallet};
//! use autonomi::client::payment::PaymentOption;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let client = Client::init().await?;
//!
//!     // Default wallet of testnet.
//!     let key = "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";
//!     let wallet = Wallet::new_from_private_key(Default::default(), key)?;
//!     let payment = PaymentOption::Wallet(wallet);
//!
//!     // Put and fetch data.
//!     let (cost, data_addr) = client.data_put_public(Bytes::from("Hello, World"), payment.clone()).await?;
//!     let _data_fetched = client.data_get_public(&data_addr).await?;
//!
//!     // Put and fetch directory from local file system.
//!     let (cost, dir_addr) = client.dir_upload_public("files/to/upload".into(), payment).await?;
//!     client.dir_download_public(&dir_addr, "files/downloaded".into()).await?;
//!
//!     Ok(())
//! }
//! ```
//!
//! # Data types
//!
//! This API gives access to two fundamental types on the network: Chunks and GraphEntry.
//!
//! When we upload data, it's split into chunks using self-encryption, yielding
//! a 'data map' allowing us to reconstruct the data again. Any two people that
//! upload the exact same data will get the same data map, as all chunks are
//! content-addressed and self-encryption is deterministic.
//!
//! # Features
//!
//! - `loud`: Print debug information to stdout

// docs.rs generation will enable unstable `doc_cfg` feature
#![cfg_attr(docsrs, feature(doc_cfg))]

#[macro_use]
extern crate tracing;

pub mod client;
pub mod self_encryption;

/// Client Operation config types
pub use ant_networking::{ResponseQuorum, RetryStrategy};

// The Network data types
pub use client::data_types::chunk;
pub use client::data_types::graph;
pub use client::data_types::pointer;
pub use client::data_types::scratchpad;

// The high-level data types
pub use client::data;
pub use client::files;
pub use client::register;
pub use client::vault;

// Re-exports of the evm types
pub use ant_evm::utils::get_evm_network;
pub use ant_evm::EvmNetwork as Network;
pub use ant_evm::EvmWallet as Wallet;
pub use ant_evm::QuoteHash;
pub use ant_evm::RewardsAddress;
pub use ant_evm::{Amount, AttoTokens};

// Re-exports of the ant-protocol address parsing error
pub use ant_protocol::storage::AddressParseError;

// Re-exports of the bls types
pub use bls::{PublicKey, SecretKey, Signature};

#[doc(no_inline)] // Place this under 'Re-exports' in the docs.
pub use bytes::Bytes;
#[doc(no_inline)] // Place this under 'Re-exports' in the docs.
pub use libp2p::Multiaddr;

#[doc(inline)]
pub use client::{
    // Client Configs
    config::ClientConfig,
    config::ClientOperatingStrategy,

    // Native data types
    data_types::chunk::Chunk,
    data_types::chunk::ChunkAddress,
    data_types::graph::GraphEntry,
    data_types::graph::GraphEntryAddress,
    data_types::pointer::Pointer,
    data_types::pointer::PointerAddress,
    data_types::scratchpad::Scratchpad,
    data_types::scratchpad::ScratchpadAddress,

    // Client
    Client,
};

#[cfg(feature = "extension-module")]
mod python;
