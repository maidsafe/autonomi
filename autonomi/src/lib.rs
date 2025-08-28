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
//! use autonomi::PaymentOption;
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
//! a 'datamap' allowing us to reconstruct the data again. Any two people that
//! upload the exact same data will get the same datamap, as all chunks are
//! content-addressed and self-encryption is deterministic.
//!
//! # Features
//!
//! - `loud`: Print debug information to stdout

// docs.rs generation will enable unstable `doc_cfg` feature
#![cfg_attr(docsrs, feature(doc_cfg))]
#![allow(clippy::large_enum_variant)]
#![allow(clippy::result_large_err)]
// Allow expect/panic and wrong_self_convention temporarily
#![allow(clippy::expect_used)]
#![allow(clippy::panic)]
#![allow(clippy::wrong_self_convention)]

#[macro_use]
extern crate tracing;

pub mod client;
pub mod self_encryption;

// The Network data types - now from client modules that build on autonomi_core
pub use client::chunk;
pub use client::graph;
pub use client::pointer;
pub use client::scratchpad;

// The high-level data types
pub use client::data;
pub use client::files;
pub use client::register;
pub use client::vault;

// Re-exports of the evm types
pub use ant_evm::EvmNetwork as Network;
pub use ant_evm::EvmWallet as Wallet;
pub use ant_evm::QuoteHash;
pub use ant_evm::RewardsAddress;
pub use ant_evm::utils::{Error as EvmUtilError, get_evm_network};
pub use ant_evm::{Amount, AttoTokens};
pub use ant_evm::{MaxFeePerGas, TransactionConfig};

// Re-exports of address related types
pub use ant_protocol::storage::AddressParseError;
pub use xor_name::XorName;

// Re-exports protocol version
pub use ant_protocol::version;

// Re-exports of the bls types
pub use bls::{PublicKey, SecretKey, Signature};

#[doc(no_inline)] // Place this under 'Re-exports' in the docs.
pub use bytes::Bytes;
#[doc(no_inline)] // Place this under 'Re-exports' in the docs.
pub use libp2p::Multiaddr;

#[doc(inline)]
pub use client::Client;

#[doc(inline)]
pub use ant_protocol::storage::{
    Chunk, ChunkAddress, GraphEntry, GraphEntryAddress, Pointer, PointerAddress, Scratchpad,
    ScratchpadAddress,
};

pub use ant_bootstrap::{InitialPeersConfig, config::BootstrapCacheConfig};
pub use ant_protocol::storage::DataTypes;

// Re-export core types for compatibility and as main API
pub use autonomi_core::{
    Addresses, ClientConfig, ClientEvent, ClientOperatingStrategy, CostError, DataContent,
    DataMapChunk, GetError, NetworkError, PaymentOption, PaymentQuote, StoreQuote, UploadSummary,
    client::{ClientInitSetup, config::BootstrapError, payment::Receipt},
};

// Re-export payment functions at root level for backward compatibility
pub use autonomi_core::client::payment::receipt_from_store_quotes;

#[cfg(feature = "extension-module")]
mod python;

// Re-export to maintain backward compatible naming space
pub mod networking {
    pub use autonomi_core::networking::{PeerId, Quorum, Record, RetryStrategy, Strategy};
}
