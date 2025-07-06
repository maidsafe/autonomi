// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("Failed to obtain any bootstrap peers")]
    NoBootstrapPeersFound,
    #[error("Failed to parse cache data")]
    FailedToParseCacheData,
    #[error("Could not obtain data directory")]
    CouldNotObtainDataDir,
    #[error("Invalid bootstrap cache directory")]
    InvalidBootstrapCacheDir,
    #[error("Could not obtain bootstrap addresses from {0} after {1} retries")]
    FailedToObtainAddrsFromUrl(String, usize),
    #[error("Failed to parse Url")]
    FailedToParseUrl,
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("Lock error")]
    LockError,
}

pub type Result<T> = std::result::Result<T, Error>;
