// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

/// Analysis error display variants - simplified mapping from AnalysisError
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AnalysisErrorDisplay {
    UnrecognizedInput,
    FailedGet,
    // GetError variants expanded
    InvalidDataMap,
    Decryption,
    Deserialization,
    Network(NetworkErrorDisplay),
    Protocol,
    RecordNotFound,
    RecordKindMismatch,
    Configuration,
    UnrecognizedDataMap,
    TooLargeForMemory,
}

impl AnalysisErrorDisplay {
    /// Map an AnalysisError to an AnalysisErrorDisplay variant
    pub fn from_analysis_error(error: &autonomi::client::analyze::AnalysisError) -> Self {
        use autonomi::client::GetError;
        use autonomi::client::analyze::AnalysisError;

        match error {
            AnalysisError::UnrecognizedInput => Self::UnrecognizedInput,
            AnalysisError::FailedGet => Self::FailedGet,
            AnalysisError::GetError(get_error) => match get_error {
                GetError::InvalidDataMap(_) => Self::InvalidDataMap,
                GetError::Decryption(_) => Self::Decryption,
                GetError::Deserialization(_) => Self::Deserialization,
                GetError::Network(network_error) => {
                    Self::Network(NetworkErrorDisplay::from_network_error(network_error))
                }
                GetError::Protocol(_) => Self::Protocol,
                GetError::RecordNotFound => Self::RecordNotFound,
                GetError::RecordKindMismatch(_) => Self::RecordKindMismatch,
                GetError::Configuration(_) => Self::Configuration,
                GetError::UnrecognizedDataMap(_) => Self::UnrecognizedDataMap,
                GetError::TooLargeForMemory(_) => Self::TooLargeForMemory,
            },
        }
    }
}

/// Network error display variants - simplified mapping from NetworkError
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkErrorDisplay {
    NetworkDriverOffline,
    NetworkDriverReceive,
    IncompatibleNetworkProtocol,
    InvalidNonZeroUsize,
    GetClosestPeersTimeout,
    InsufficientPeers,
    PutRecordMissingTargets,
    PutRecordVerification,
    PutRecordQuorumFailed,
    PutRecordTooManyPeerFailed,
    PutRecordTimeout,
    PutRecordRejected,
    OutdatedRecordRejected,
    GetQuoteError,
    InvalidQuote,
    InsufficientQuotes,
    SplitRecord,
    GetRecordTimeout,
    GetRecordQuorumFailed,
    GetRecordError,
    InvalidRetryStrategy,
    InvalidNodeMerkleCandidate,
    TopologyVerificationFailed,
}

impl NetworkErrorDisplay {
    /// Map a NetworkError to a NetworkErrorDisplay variant
    pub fn from_network_error(error: &autonomi::networking::NetworkError) -> Self {
        use autonomi::networking::NetworkError;

        match error {
            NetworkError::NetworkDriverOffline => Self::NetworkDriverOffline,
            NetworkError::NetworkDriverReceive(_) => Self::NetworkDriverReceive,
            NetworkError::IncompatibleNetworkProtocol => Self::IncompatibleNetworkProtocol,
            NetworkError::InvalidNonZeroUsize(_) => Self::InvalidNonZeroUsize,
            NetworkError::GetClosestPeersTimeout => Self::GetClosestPeersTimeout,
            NetworkError::InsufficientPeers { .. } => Self::InsufficientPeers,
            NetworkError::PutRecordMissingTargets => Self::PutRecordMissingTargets,
            NetworkError::PutRecordVerification(_) => Self::PutRecordVerification,
            NetworkError::PutRecordQuorumFailed(_, _) => Self::PutRecordQuorumFailed,
            NetworkError::PutRecordTooManyPeerFailed(_, _) => Self::PutRecordTooManyPeerFailed,
            NetworkError::PutRecordTimeout(_) => Self::PutRecordTimeout,
            NetworkError::PutRecordRejected(_) => Self::PutRecordRejected,
            NetworkError::OutdatedRecordRejected { .. } => Self::OutdatedRecordRejected,
            NetworkError::GetQuoteError(_) => Self::GetQuoteError,
            NetworkError::InvalidQuote(_) => Self::InvalidQuote,
            NetworkError::InsufficientQuotes { .. } => Self::InsufficientQuotes,
            NetworkError::SplitRecord(_) => Self::SplitRecord,
            NetworkError::GetRecordTimeout(_) => Self::GetRecordTimeout,
            NetworkError::GetRecordQuorumFailed { .. } => Self::GetRecordQuorumFailed,
            NetworkError::GetRecordError(_) => Self::GetRecordError,
            NetworkError::InvalidRetryStrategy => Self::InvalidRetryStrategy,
            NetworkError::InvalidNodeMerkleCandidate(_) => Self::InvalidNodeMerkleCandidate,
            NetworkError::TopologyVerificationFailed { .. } => Self::TopologyVerificationFailed,
        }
    }
}
