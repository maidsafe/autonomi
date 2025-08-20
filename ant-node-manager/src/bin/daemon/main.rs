// Copyright (C) 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

#[macro_use]
extern crate tracing;

use ant_logging::LogBuilder;
use ant_node_manager::{DAEMON_DEFAULT_PORT, config::get_node_registry_path};
use ant_service_management::{
    NodeRegistryManager,
    antctl_proto::{
        GetStatusRequest, GetStatusResponse, NodeServiceRestartRequest, NodeServiceRestartResponse,
        ant_ctl_server::{AntCtl, AntCtlServer},
        get_status_response::Node,
    },
};
use clap::Parser;
use color_eyre::eyre::{Result, eyre};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use tonic::{Code, Request, Response, Status, transport::Server};
use tracing::Level;

#[derive(Parser, Debug)]
#[command(disable_version_flag = true)]
struct Args {
    /// Specify an Ipv4Addr for the daemon to listen on. This is useful if you want to manage the nodes remotely.
    ///
    /// If not set, the daemon listens locally for commands.
    #[clap(long, default_value_t = Ipv4Addr::new(127, 0, 0, 1))]
    address: Ipv4Addr,
    /// Print the crate version.
    #[clap(long)]
    pub crate_version: bool,
    /// Print the package version.
    #[cfg(not(feature = "nightly"))]
    #[clap(long)]
    pub package_version: bool,
    /// Specify a port for the daemon to listen for RPCs. It defaults to 12500 if not set.
    #[clap(long, default_value_t = DAEMON_DEFAULT_PORT)]
    port: u16,
    /// Print version information.
    #[clap(long)]
    version: bool,
}

struct AntCtlDaemon {}

// Implementing RPC interface for service defined in .proto
#[tonic::async_trait]
impl AntCtl for AntCtlDaemon {
    async fn restart_node_service(
        &self,
        request: Request<NodeServiceRestartRequest>,
    ) -> Result<Response<NodeServiceRestartResponse>, Status> {
        println!("RPC request received {:?}", request.get_ref());
        info!("RPC request received {:?}", request.get_ref());

        info!("no-op for rpc request");
        Ok(Response::new(NodeServiceRestartResponse {}))
    }

    async fn get_status(
        &self,
        request: Request<GetStatusRequest>,
    ) -> Result<Response<GetStatusResponse>, Status> {
        println!("RPC request received {:?}", request.get_ref());
        info!("RPC request received {:?}", request.get_ref());
        let node_registry = Self::load_node_registry().await.map_err(|err| {
            Status::new(
                Code::Internal,
                format!("Failed to load node registry: {err}"),
            )
        })?;

        let mut nodes_info = Vec::new();
        for node in node_registry.nodes.read().await.iter() {
            let node = node.read().await;

            nodes_info.push(Node {
                peer_id: node.peer_id.map(|id| id.to_bytes()),
                status: node.status.clone() as i32,
                number: node.number as u32,
            });
        }

        info!("Node status retrieved, nod len: {:?}", nodes_info.len());
        Ok(Response::new(GetStatusResponse { nodes: nodes_info }))
    }
}

impl AntCtlDaemon {
    async fn load_node_registry() -> Result<NodeRegistryManager> {
        let node_registry_path = get_node_registry_path()
            .map_err(|err| eyre!("Could not obtain node registry path: {err:?}"))?;
        let node_registry = NodeRegistryManager::load(&node_registry_path)
            .await
            .map_err(|err| eyre!("Could not load node registry: {err:?}"))?;
        Ok(node_registry)
    }
}

// The SafeNodeManager trait returns `Status` as its error. So the actual logic is here and we can easily map the errors
// into Status inside the trait fns.
impl AntCtlDaemon {}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let args = Args::parse();

    if args.version {
        println!(
            "{}",
            ant_build_info::version_string(
                "Autonomi Node Manager RPC Daemon",
                env!("CARGO_PKG_VERSION"),
                None
            )
        );
        return Ok(());
    }

    if args.crate_version {
        println!("{}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    #[cfg(not(feature = "nightly"))]
    if args.package_version {
        println!("{}", ant_build_info::package_version());
        return Ok(());
    }

    let _log_handles = get_log_builder()?.initialize()?;
    println!("Starting antctld");
    let service = AntCtlDaemon {};

    if let Err(err) = Server::builder()
        .add_service(AntCtlServer::new(service))
        .serve(SocketAddr::new(IpAddr::V4(args.address), args.port))
        .await
    {
        error!("Antctl Daemon failed to start: {err:?}");
        println!("Antctl Daemon failed to start: {err:?}");
        return Err(err.into());
    }

    Ok(())
}

fn get_log_builder() -> Result<LogBuilder> {
    let logging_targets = vec![
        ("ant_node_manager".to_string(), Level::TRACE),
        ("antctl".to_string(), Level::TRACE),
        ("antctld".to_string(), Level::TRACE),
        ("ant_service_management".to_string(), Level::TRACE),
    ];
    let timestamp = chrono::Local::now().format("%Y-%m-%d_%H-%M-%S").to_string();

    let output_dest = dirs_next::data_dir()
        .ok_or_else(|| eyre!("Could not obtain user data directory"))?
        .join("autonomi")
        .join("antctld")
        .join("logs")
        .join(format!("log_{timestamp}"));

    let mut log_builder = LogBuilder::new(logging_targets);
    log_builder.output_dest(ant_logging::LogOutputDest::Path(output_dest));
    Ok(log_builder)
}
