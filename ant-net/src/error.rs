// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! Error types for the ant-net crate.

use thiserror::Error;

/// Result type used throughout ant-net.
pub type Result<T> = std::result::Result<T, AntNetError>;

/// Main error type for ant-net operations.
#[derive(Debug, Error)]
pub enum AntNetError {
    /// Transport layer errors
    #[error("Transport error: {0}")]
    Transport(String),

    /// Connection management errors
    #[error("Connection error: {0}")]
    Connection(String),

    /// Protocol handling errors
    #[error("Protocol error: {0}")]
    Protocol(String),

    /// Behavior composition errors
    #[error("Behavior error: {0}")]
    Behavior(String),

    /// Network driver errors
    #[error("Driver error: {0}")]
    Driver(String),

    /// Configuration errors
    #[error("Configuration error: {0}")]
    Configuration(String),

    /// Network event handling errors
    #[error("Event error: {0}")]
    Event(String),

    /// Generic I/O errors
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Timeout errors
    #[error("Operation timed out: {0}")]
    Timeout(String),

    /// Channel communication errors
    #[error("Channel error: {0}")]
    Channel(String),

    /// Serialization/deserialization errors
    #[error("Serialization error: {0}")]
    Serialization(String),

    /// Generic network errors
    #[error("Network error: {0}")]
    Network(String),
}

impl From<tokio::sync::oneshot::error::RecvError> for AntNetError {
    fn from(err: tokio::sync::oneshot::error::RecvError) -> Self {
        AntNetError::Channel(err.to_string())
    }
}

impl<T> From<tokio::sync::mpsc::error::SendError<T>> for AntNetError {
    fn from(err: tokio::sync::mpsc::error::SendError<T>) -> Self {
        AntNetError::Channel(err.to_string())
    }
}

impl From<libp2p::TransportError<std::io::Error>> for AntNetError {
    fn from(err: libp2p::TransportError<std::io::Error>) -> Self {
        AntNetError::Transport(err.to_string())
    }
}

impl From<libp2p::swarm::DialError> for AntNetError {
    fn from(err: libp2p::swarm::DialError) -> Self {
        AntNetError::Connection(err.to_string())
    }
}

impl From<libp2p::noise::Error> for AntNetError {
    fn from(err: libp2p::noise::Error) -> Self {
        AntNetError::Transport(err.to_string())
    }
}