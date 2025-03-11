// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::commands::SubCmd;
use ant_logging::{LogFormat, LogOutputDest};
use autonomi::PeersArgs;
use clap::Parser;
use color_eyre::Result;
use std::time::Duration;

// Please do not remove the blank lines in these doc comments.
// They are used for inserting line breaks when the help menu is rendered in the UI.
#[derive(Parser)]
#[command(disable_version_flag = true)]
#[command(author, version, about, long_about = None)]
pub(crate) struct Opt {
    // Available subcommands. This is optional to allow `--version` to work without a subcommand.
    #[clap(subcommand)]
    pub command: Option<SubCmd>,

    /// The maximum duration to wait for a connection to the network before timing out.
    #[clap(long = "timeout", global = true, value_parser = |t: &str| -> Result<Duration> { Ok(t.parse().map(Duration::from_secs)?) })]
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
    #[clap(long, value_parser = LogOutputDest::parse_from_str, verbatim_doc_comment, default_value = "data-dir")]
    pub log_output_dest: LogOutputDest,

    /// Specify the network ID to use. This will allow you to run the CLI on a different network.
    ///
    /// By default, the network ID is set to 1, which represents the mainnet.
    #[clap(long, verbatim_doc_comment)]
    pub network_id: Option<u8>,

    /// Prevent verification of data storage on the network.
    ///
    /// This may increase operation speed, but offers no guarantees that operations were successful.
    #[clap(global = true, long = "no-verify", short = 'x')]
    pub no_verify: bool,

    #[command(flatten)]
    pub(crate) peers: PeersArgs,

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
