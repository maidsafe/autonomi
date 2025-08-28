// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use ant_evm::payment_vault::MAX_TRANSFERS_PER_TRANSACTION;
pub use autonomi_core::{ClientConfig, ClientOperatingStrategy};
use std::sync::LazyLock;

/// Maximum number of chunks that we allow to download from a datamap in memory.
/// This affects the maximum size of data downloaded with APIs such as [`crate::Client::data_get`]
///
/// Can be overridden by the `MAX_IN_MEMORY_DOWNLOAD_SIZE ` environment variable.
pub static MAX_IN_MEMORY_DOWNLOAD_SIZE: LazyLock<usize> = LazyLock::new(|| {
    let size = std::env::var("MAX_IN_MEMORY_DOWNLOAD_SIZE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(20);
    info!("Max in memory download size: {}", size);
    size
});

/// Number of files to upload in parallel.
///
/// Can be overridden by the `FILE_UPLOAD_BATCH_SIZE` environment variable.
pub static FILE_UPLOAD_BATCH_SIZE: LazyLock<usize> = LazyLock::new(|| {
    let batch_size = std::env::var("FILE_UPLOAD_BATCH_SIZE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);
    info!("File upload batch size: {}", batch_size);
    batch_size
});

/// Number of batch size of an entire quote-pay-upload flow to process.
/// Suggested to be multiples of `MAX_TRANSFERS_PER_TRANSACTION  / 3` (records-payouts-per-transaction).
///
/// Can be overridden by the `UPLOAD_FLOW_BATCH_SIZE` environment variable.
pub(crate) static UPLOAD_FLOW_BATCH_SIZE: LazyLock<usize> = LazyLock::new(|| {
    let batch_size = std::env::var("UPLOAD_FLOW_BATCH_SIZE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(MAX_TRANSFERS_PER_TRANSACTION / 3);
    info!("Upload flow batch size: {}", batch_size);
    batch_size
});
