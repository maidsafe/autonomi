// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use thiserror::Error;

#[derive(Debug, Error)]
#[allow(dead_code)]
pub enum UpgradeError {
    #[error("Already running latest version")]
    AlreadyLatest,

    #[error("Failed to fetch release info: {0}")]
    FetchReleaseInfoFailed(String),

    #[error("No binaries found for platform: {0}")]
    PlatformBinariesNotFound(String),

    #[error("Binary download failed: {0}")]
    DownloadFailed(String),

    #[error("Binary hash verification failed")]
    HashVerificationFailed,

    #[error("Binary replacement failed: {0}")]
    BinaryReplacementFailed(String),

    #[error("Failed to manage upgrade lock: {0}")]
    LockError(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Release error: {0}")]
    Release(#[from] ant_releases::Error),

    #[error("Version parsing error: {0}")]
    VersionParsing(#[from] semver::Error),
}

pub type Result<T> = std::result::Result<T, UpgradeError>;
