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
    #[error("No peers found: {0}")]
    NoPeersFound(String),
    #[error("Invalid response: {0}")]
    InvalidResponse(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("Timeout error: {0}")]
    Timeout(#[from] tokio::time::error::Elapsed),
    #[error("Persist error: {0}")]
    Persist(#[from] tempfile::PersistError),
    #[error("Lock error")]
    LockError,
    #[error("Circuit breaker open: {0}")]
    CircuitBreakerOpen(String),
    #[error("Request failed: {0}")]
    RequestFailed(String),
    #[error("Request timeout")]
    RequestTimeout,
    #[error("Invalid multiaddr: {0}")]
    InvalidMultiAddr(#[from] libp2p::multiaddr::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
