// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! Core Autonomi client API - simplified interface for network operations.
//!
//! This crate provides the core functionality needed to interact with the Autonomi network
//! through a simplified API. The `autonomi` crate builds on top of this to provide the
//! full high-level API.

#![cfg_attr(docsrs, feature(doc_cfg))]
#![allow(clippy::large_enum_variant)]
#![allow(clippy::result_large_err)]

#[macro_use]
extern crate tracing;

pub mod client;
pub mod networking;

// Re-exports of core types
pub use ant_evm::EvmNetwork as Network;
pub use ant_evm::{Amount, AttoTokens};
pub use ant_protocol::storage::AddressParseError;
pub use xor_name::XorName;

#[doc(no_inline)]
pub use bytes::Bytes;
#[doc(no_inline)]
pub use libp2p::Multiaddr;

pub use client::quote::CostError;
#[doc(inline)]
pub use client::{
    Client, ClientEvent, ClientInitSetup, ConnectError, DataContent, Error, GetError, PutError,
    UploadSummary,
};

pub use client::config::{ClientConfig, ClientOperatingStrategy};

// Re-exports of networking types
pub use networking::{NetworkAddress, NetworkError, PaymentQuote, StoreQuote};

// Re-exports of data types
pub use client::data_types::chunk::{
    CHUNK_DOWNLOAD_BATCH_SIZE, CHUNK_UPLOAD_BATCH_SIZE, Chunk, ChunkAddress, DataMapChunk,
};
pub use client::payment::PaymentOption;
pub use client::upload::EncryptionStream;
pub use client::utils::process_tasks_with_max_concurrency;

pub use networking::common::Addresses;

// Re-exports of cryptographic types
pub use bls::{PublicKey, SecretKey, Signature};
