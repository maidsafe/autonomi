// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::commands::SubCmd;
use ant_logging::{LogFormat, LogOutputDest};
use autonomi::InitialPeersConfig;
use autonomi::Network as EvmNetwork;
use autonomi::get_evm_network;
use clap::Parser;
use color_eyre::Result;
use std::time::Duration;

pub(crate) const LOCAL_NETWORK_ID: u8 = 0;
pub(crate) const MAIN_NETWORK_ID: u8 = 1;
pub(crate) const ALPHA_NETWORK_ID: u8 = 2;

// Please do not remove the blank lines in these doc comments.
// They are used for inserting line breaks when the help menu is rendered in the UI.

/// Network identifier for selecting which network to connect to.
///
/// Predefined networks have reserved IDs:
/// - 0: Local Network
/// - 1: Mainnet
/// - 2: Alpha Network
///
/// Custom networks can use any ID from 3 to 255.
#[derive(Debug, Clone, Copy)]
pub struct NetworkId {
    id: u8,
}

impl NetworkId {
    /// Create a new NetworkId with the specified ID.
    pub fn new(id: u8) -> Self {
        Self { id }
    }

    /// Create a new NetworkId for the local network (ID 0).
    pub fn local() -> Self {
        Self::new(LOCAL_NETWORK_ID)
    }

    /// Create a new NetworkId for the alpha network (ID 2).
    pub fn alpha() -> Self {
        Self::new(ALPHA_NETWORK_ID)
    }

    /// Get the raw ID value.
    pub fn as_u8(&self) -> u8 {
        self.id
    }

    /// Get the EVM network corresponding to this network ID.
    #[allow(dead_code)]
    pub fn evm_network(&self, local: bool) -> Result<EvmNetwork> {
        match self.id {
            0 => Ok(EvmNetwork::new(true)?),
            1 => Ok(EvmNetwork::default()),
            2 => Ok(EvmNetwork::ArbitrumSepoliaTest),
            _ => Ok(get_evm_network(local, Some(self.id))?),
        }
    }
}

impl std::fmt::Display for NetworkId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.id)
    }
}

impl std::str::FromStr for NetworkId {
    type Err = color_eyre::eyre::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.parse::<u8>() {
            Ok(id) => Ok(NetworkId::new(id)),
            Err(_) => Err(color_eyre::eyre::eyre!(
                "Invalid network ID: must be a number from 0-255"
            )),
        }
    }
}

#[derive(Parser)]
#[command(disable_version_flag = true)]
#[command(author, version, about, long_about = None)]
pub(crate) struct Opt {
    /// Set to connect to the alpha network.
    #[clap(long)]
    pub alpha: bool,

    // Available subcommands. This is optional to allow `--version` to work without a subcommand.
    #[clap(subcommand)]
    pub command: Option<SubCmd>,

    /// The maximum duration to wait for a connection to the network before timing out.
    #[clap(long = "timeout", global = true, value_parser = |t: &str| -> Result<Duration> { Ok(t.parse().map(Duration::from_secs)?) }
    )]
    pub connection_timeout: Option<Duration>,

    /// Print the crate version.
    #[clap(long)]
    pub crate_version: bool,

    /// Specify the logging format.
    ///
    /// Valid values are "default" or "json".
    ///
    /// If the argument is not used, the default format will be applied.
    #[clap(long, value_parser = LogFormat::parse_from_str, verbatim_doc_comment)]
    pub log_format: Option<LogFormat>,

    /// Specify the logging output destination.
    ///
    /// Valid values are "stdout", "data-dir", or a custom path.
    ///
    /// `data-dir` is the default value.
    ///
    /// The data directory location is platform specific:
    ///  - Linux: $HOME/.local/share/autonomi/client/logs
    ///  - macOS: $HOME/Library/Application Support/autonomi/client/logs
    ///  - Windows: C:\Users\<username>\AppData\Roaming\autonomi\client\logs
    #[allow(rustdoc::invalid_html_tags)]
    #[clap(long, value_parser = LogOutputDest::parse_from_str, verbatim_doc_comment, default_value = "data-dir"
    )]
    pub log_output_dest: LogOutputDest,

    /// Specify the network ID to use. This will allow you to run the CLI on a different network.
    /// Note that this overrides all other network config options (except in the Custom Network case).
    ///
    /// Valid values are:
    ///  - 0: Local Network
    ///  - 1: Mainnet (default)
    ///  - 2: Alpha Network
    ///  - 3-255: Custom Networks (obtained through environment variables and other network config flags)
    #[clap(long, verbatim_doc_comment, default_value = "1")]
    pub network_id: NetworkId,

    /// Prevent verification of data storage on the network.
    ///
    /// This may increase operation speed, but offers no guarantees that operations were successful.
    #[clap(global = true, long = "no-verify", short = 'x')]
    pub no_verify: bool,

    #[command(flatten)]
    pub(crate) peers: InitialPeersConfig,

    /// Print the package version.
    #[cfg(not(feature = "nightly"))]
    #[clap(long)]
    pub package_version: bool,

    /// Print the network protocol version.
    #[clap(long)]
    pub protocol_version: bool,

    /// Print version information.
    #[clap(long)]
    pub version: bool,
}
