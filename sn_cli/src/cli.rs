// Copyright 2023 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use clap::Parser;
use libp2p::Multiaddr;

use crate::subcommands::SubCmd;

// Please do not remove the blank lines in these doc comments.
// They are used for inserting line breaks when the help menu is rendered in the UI.
#[derive(Parser)]
#[command(author, version, about, long_about = None)]
pub(crate) struct Opt {
    /// Provide a peer to connect to a public network, using the MultiAddr format.
    ///
    /// An example MultiAddr:
    ///
    /// /ip4/13.40.152.226/udp/12000/quic-v1/p2p/12D3KooWRi6wF7yxWLuPSNskXc6kQ5cJ6eaymeMbCRdTnMesPgFx
    ///
    /// Noteworthy are the second, fourth, and last parts.
    ///
    /// Those are the IP address and UDP port the peer is listening on, and its peer ID, respectively.
    ///
    /// Many peers can be provided by using the argument multiple times.
    ///
    /// If none are provided, a connection will be attempted to a local network.
    #[clap(long = "peer", global = true)]
    pub peers: Vec<Multiaddr>,

    /// Available sub commands.
    #[clap(subcommand)]
    pub cmd: SubCmd,

    /// Timeout in seconds for the CLI to wait for a data response from the network.
    #[clap(long = "timeout", global = true)]
    pub timeout: Option<u64>,
}
