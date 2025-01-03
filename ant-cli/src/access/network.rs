// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use ant_bootstrap::{PeersArgs, ANT_PEERS_ENV};
use autonomi::Multiaddr;
use color_eyre::eyre::Context;
use color_eyre::Result;
use color_eyre::Section;

pub enum NetworkPeers {
    Local(Vec<Multiaddr>),
    Public(Vec<Multiaddr>),
}

impl NetworkPeers {
    pub fn peers(&self) -> &Vec<Multiaddr> {
        match self {
            NetworkPeers::Local(addrs) => addrs,
            NetworkPeers::Public(addrs) => addrs,
        }
    }

    pub fn is_local(&self) -> bool {
        matches!(self, NetworkPeers::Local(_))
    }
}

pub async fn get_peers(peers: PeersArgs) -> Result<NetworkPeers> {
    let addrs = peers.get_addrs(None, Some(100)).await
        .wrap_err("Please provide valid Network peers to connect to")
        .with_suggestion(|| format!("make sure you've provided network peers using the --peers option or the {ANT_PEERS_ENV} env var"))
        .with_suggestion(|| "a peer address looks like this: /ip4/42.42.42.42/udp/4242/quic-v1/p2p/B64nodePeerIDvdjb3FAJF4ks3moreBase64CharsHere")?;

    let net = if peers.local {
        NetworkPeers::Local(addrs)
    } else {
        NetworkPeers::Public(addrs)
    };

    Ok(net)
}
