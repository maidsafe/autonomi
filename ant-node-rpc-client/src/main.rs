// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.
//

use ant_logging::{Level, LogBuilder};
use ant_node::NodeEvent;
use ant_protocol::antnode_proto::{NodeEventsRequest, ant_node_client::AntNodeClient};
use ant_service_management::rpc::{RpcActions, RpcClient};
use clap::Parser;
use color_eyre::eyre::Result;
use std::{net::SocketAddr, time::Duration};
use tokio_stream::StreamExt;
use tonic::Request;

#[derive(Parser, Debug)]
#[command(disable_version_flag = true)]
struct Opt {
    /// Address of the node's RPC service, e.g. 127.0.0.1:12001.
    addr: SocketAddr,
    /// subcommands
    #[clap(subcommand)]
    cmd: Cmd,

    /// Print the crate version.
    #[clap(long)]
    crate_version: bool,

    /// Print the package version.
    #[cfg(not(feature = "nightly"))]
    #[clap(long)]
    package_version: bool,

    /// Print version information.
    #[clap(long)]
    version: bool,
}

#[derive(Parser, Debug)]
enum Cmd {
    /// Retrieve information about the node itself
    #[clap(name = "info")]
    Info,
    /// Retrieve information about the node's connections to the network
    #[clap(name = "netinfo")]
    Netinfo,
    /// Start listening for node events.
    /// Note this blocks the app and it will print events as they are broadcasted by the node
    #[clap(name = "events")]
    Events,
    /// Restart the node after the specified delay
    #[clap(name = "restart")]
    Restart {
        /// Delay in milliseconds before restartng the node
        #[clap(default_value = "0")]
        delay_millis: u64,
        /// Retain the node's PeerId by reusing the same root dir.
        retain_peer_id: bool,
    },
    /// Stop the node after the specified delay
    #[clap(name = "stop")]
    Stop {
        /// Delay in milliseconds before stopping the node
        #[clap(default_value = "0")]
        delay_millis: u64,
    },
    /// Update to latest `antnode` released version, and restart it
    #[clap(name = "update")]
    Update {
        /// Delay in milliseconds before updating and restarting the node
        #[clap(default_value = "0")]
        delay_millis: u64,
    },
    /// Update the node's log levels.
    #[clap(name = "log")]
    Log {
        /// Change the log level of antnode. This accepts a comma-separated list of log levels for
        /// different modules or specific keywords like "all" or "v".
        ///
        /// Example: --level libp2p=DEBUG,tokio=INFO,all,sn_client=ERROR
        #[clap(name = "level", long)]
        log_level: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    // For client, default to log to std::out
    let logging_targets = vec![
        ("antnode".to_string(), Level::INFO),
        ("ant_node".to_string(), Level::INFO),
    ];
    let _log_appender_guard = LogBuilder::new(logging_targets).initialize()?;

    let opt = Opt::parse();

    if opt.version {
        println!(
            "{}",
            ant_build_info::version_string(
                "Autonomi Node RPC Client",
                env!("CARGO_PKG_VERSION"),
                None
            )
        );
    }

    if opt.crate_version {
        println!("Crate version: {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    #[cfg(not(feature = "nightly"))]
    if opt.package_version {
        println!("Package version: {}", ant_build_info::package_version());
        return Ok(());
    }

    let addr = opt.addr;

    match opt.cmd {
        Cmd::Info => node_info(addr).await,
        Cmd::Netinfo => network_info(addr).await,
        Cmd::Events => node_events(addr).await,
        Cmd::Restart {
            delay_millis,
            retain_peer_id,
        } => node_restart(addr, delay_millis, retain_peer_id).await,
        Cmd::Stop { delay_millis } => node_stop(addr, delay_millis).await,
        Cmd::Update { delay_millis } => node_update(addr, delay_millis).await,
        Cmd::Log { log_level } => update_log_level(addr, log_level).await,
    }
}

pub async fn node_info(addr: SocketAddr) -> Result<()> {
    let endpoint = format!("https://{addr}");
    let client = RpcClient::new(&endpoint);
    let node_info = client.node_info().await?;

    println!("Node info:");
    println!("==========");
    println!("RPC endpoint: {endpoint}");
    println!("Peer Id: {}", node_info.peer_id);
    println!("Logs dir: {}", node_info.log_path.to_string_lossy());
    println!("PID: {}", node_info.pid);
    println!("Binary version: {}", node_info.version);
    println!("Time since last restart: {:?}", node_info.uptime);

    Ok(())
}

pub async fn network_info(addr: SocketAddr) -> Result<()> {
    let endpoint = format!("https://{addr}");
    let client = RpcClient::new(&endpoint);
    let network_info = client.network_info().await?;

    println!("Node's connections to the Network:");
    println!();
    println!("Connected peers:");
    for peer_id in network_info.connected_peers.iter() {
        println!("Peer: {peer_id}");
    }

    println!();
    println!("Node's listeners:");
    for multiaddr_str in network_info.listeners.iter() {
        println!("Listener: {multiaddr_str}");
    }

    Ok(())
}

pub async fn node_events(addr: SocketAddr) -> Result<()> {
    let endpoint = format!("https://{addr}");
    let mut client = AntNodeClient::connect(endpoint).await?;
    let response = client
        .node_events(Request::new(NodeEventsRequest {}))
        .await?;

    println!("Listening to node events... (press Ctrl+C to exit)");

    let mut stream = response.into_inner();
    while let Some(Ok(e)) = stream.next().await {
        match NodeEvent::from_bytes(&e.event) {
            Ok(event) => println!("New event received: {event:?}"),
            Err(_) => {
                println!("Error while parsing received NodeEvent");
            }
        }
    }

    Ok(())
}

pub async fn record_addresses(addr: SocketAddr) -> Result<()> {
    let endpoint = format!("https://{addr}");
    let client = RpcClient::new(&endpoint);
    let record_addresses = client.record_addresses().await?;

    println!("Records held by the node:");
    for address in record_addresses.iter() {
        println!("Key: {:?}", address.key);
    }

    Ok(())
}

pub async fn node_restart(addr: SocketAddr, delay_millis: u64, retain_peer_id: bool) -> Result<()> {
    let endpoint = format!("https://{addr}");
    let client = RpcClient::new(&endpoint);
    client.node_restart(delay_millis, retain_peer_id).await?;
    println!(
        "Node successfully received the request to restart in {:?}",
        Duration::from_millis(delay_millis)
    );
    Ok(())
}

pub async fn node_stop(addr: SocketAddr, delay_millis: u64) -> Result<()> {
    let endpoint = format!("https://{addr}");
    let client = RpcClient::new(&endpoint);
    client.node_stop(delay_millis).await?;
    println!(
        "Node successfully received the request to stop in {:?}",
        Duration::from_millis(delay_millis)
    );
    Ok(())
}

pub async fn node_update(addr: SocketAddr, delay_millis: u64) -> Result<()> {
    let endpoint = format!("https://{addr}");
    let client = RpcClient::new(&endpoint);
    client.node_update(delay_millis).await?;
    println!(
        "Node successfully received the request to try to update in {:?}",
        Duration::from_millis(delay_millis)
    );
    Ok(())
}

pub async fn update_log_level(addr: SocketAddr, log_levels: String) -> Result<()> {
    let endpoint = format!("https://{addr}");
    let client = RpcClient::new(&endpoint);

    client.update_log_level(log_levels.clone()).await?;
    println!("Node successfully received the request to update the log level to {log_levels:?}",);
    Ok(())
}
