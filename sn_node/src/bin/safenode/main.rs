// Copyright 2023 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

#[macro_use]
extern crate tracing;

mod rpc;

use sn_logging::init_logging;
#[cfg(feature = "metrics")]
use sn_logging::metrics::init_metrics;
use sn_node::{Marker, Node, NodeEvent, NodeEventsReceiver};
use sn_peers_acquisition::{parse_peer_addr, PeersArgs};

use clap::Parser;
use eyre::{eyre, Error, Result};
use libp2p::{identity::Keypair, Multiaddr, PeerId};
use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::{
    fs::File,
    io::AsyncWriteExt,
    runtime::Runtime,
    sync::{broadcast::error::RecvError, mpsc},
    time::sleep,
};
use tracing_core::Level;

// Please do not remove the blank lines in these doc comments.
// They are used for inserting line breaks when the help menu is rendered in the UI.
#[derive(Parser, Debug)]
#[clap(name = "safenode cli", version = env!("CARGO_PKG_VERSION"))]
struct Opt {
    /// Specify the node's logging output directory.
    ///
    /// If not provided, logging will go to stdout.
    #[clap(long)]
    log_dir: Option<PathBuf>,

    /// Specify the node's data directory.
    ///
    /// If not provided, the default location is platform specific:
    ///  - Linux: $HOME/.local/share/safe/node/<peer-id>
    ///  - macOS: $HOME/Library/Application Support/safe/node/<peer-id>
    ///  - Windows: C:\Users\{username}\AppData\Roaming\safe\node\<peer-id>
    #[allow(rustdoc::invalid_html_tags)]
    #[clap(long, verbatim_doc_comment)]
    root_dir: Option<PathBuf>,

    /// Specify the port to listen on.
    ///
    /// The special value `0` will cause the OS to assign a random port.
    #[clap(long, default_value_t = 0)]
    port: u16,

    /// Specify the IP to listen on.
    ///
    /// The special value `0.0.0.0` binds to all network interfaces available.
    #[clap(long, default_value_t = IpAddr::V4(Ipv4Addr::UNSPECIFIED))]
    ip: IpAddr,

    #[command(flatten)]
    peers: PeersArgs,

    /// Enable the admin/control RPC service by providing an IP and port for it to listen on.
    ///
    /// The RPC service can be used for querying information about the running node.
    #[clap(long)]
    rpc: Option<SocketAddr>,

    /// Run the node in local mode.
    ///
    /// When this flag is set, we will not filter out local addresses that we observe.
    #[clap(long)]
    local: bool,

    /// Use JSON for logging output.
    ///
    /// Only applies when --log-dir is also set to output logs to file.
    #[clap(long)]
    json_log_output: bool,
}

#[derive(Debug)]
// To be sent to the main thread in order to stop/restart the execution of the safenode app.
enum NodeCtrl {
    // Request to stop the exeution of the safenode app, providing an error as a reason for it.
    Stop { delay: Duration, cause: Error },
    // Request to restart the exeution of the safenode app,
    // retrying to join the network, after the requested delay.
    Restart(Duration),
    // Request to update the safenode app, and restart it, after the requested delay.
    Update(Duration),
}

fn main() -> Result<()> {
    let opt = Opt::parse();
    let logging_targets = vec![
        ("safenode".to_string(), Level::INFO),
        ("sn_transfers".to_string(), Level::INFO),
        ("sn_networking".to_string(), Level::INFO),
        ("sn_node".to_string(), Level::INFO),
    ];
    #[cfg(not(feature = "otlp"))]
    let _log_appender_guard = init_logging(logging_targets, &opt.log_dir, opt.json_log_output)?;
    #[cfg(feature = "otlp")]
    let (_rt, _log_appender_guard) = {
        // init logging in a separate runtime if we are sending traces to an opentelemetry server
        let rt = Runtime::new()?;
        let guard = rt
            .block_on(async { init_logging(logging_targets, &opt.log_dir, opt.json_log_output) })?;
        (rt, guard)
    };

    debug!("Built with git version: {}", sn_build_info::git_info());

    if opt.peers.peers.is_empty() {
        if !cfg!(feature = "local-discovery") {
            warn!("No peers given. As `local-discovery` feature is disabled, we will not be able to connect to the network.");
        } else {
            info!("No peers given. As `local-discovery` feature is enabled, we will attempt to connect to the network using mDNS.");
        }
    }

    let log_dir = if let Some(path) = opt.log_dir {
        format!("{}", path.display())
    } else {
        "stdout".to_string()
    };

    let node_socket_addr = SocketAddr::new(opt.ip, opt.port);

    let mut initial_peers = opt.peers.peers.clone();

    loop {
        let msg = format!(
            "Running {} v{}",
            env!("CARGO_BIN_NAME"),
            env!("CARGO_PKG_VERSION")
        );
        info!("\n{}\n{}", msg, "=".repeat(msg.len()));
        info!("Node started with initial_peers {initial_peers:?}");

        // Create a tokio runtime per `start_node` attempt, this ensures
        // any spawned tasks are closed before this would be run again.
        let rt = Runtime::new()?;
        #[cfg(feature = "metrics")]
        rt.spawn(init_metrics(std::process::id()));
        rt.block_on(start_node(
            node_socket_addr,
            initial_peers.clone(),
            opt.rpc,
            opt.local,
            &log_dir,
            opt.root_dir.clone(),
        ))?;

        // actively shut down the runtime
        rt.shutdown_timeout(Duration::from_secs(2));

        // The original passed in peers may got restarted as well.
        // Hence, try to parse from env_var and add as initial peers,
        // if not presented yet.
        if !cfg!(feature = "local-discovery") {
            match std::env::var("SAFE_PEERS") {
                Ok(str) => match parse_peer_addr(&str) {
                    Ok(peer) => {
                        if !initial_peers
                            .iter()
                            .any(|existing_peer| *existing_peer == peer)
                        {
                            initial_peers.push(peer);
                        }
                    }
                    Err(err) => error!("Cann't parse SAFE_PEERS {str:?} with error {err:?}"),
                },
                Err(err) => error!("Cann't get env var SAFE_PEERS with error {err:?}"),
            }
        }
    }
}

async fn start_node(
    node_socket_addr: SocketAddr,
    peers: Vec<Multiaddr>,
    rpc: Option<SocketAddr>,
    local: bool,
    log_dir: &str,
    root_dir: Option<PathBuf>,
) -> Result<()> {
    let started_instant = std::time::Instant::now();

    let (root_dir, keypair) = get_root_dir_and_keypair(root_dir).await?;

    info!("Starting node ...");
    let running_node = Node::run(keypair, node_socket_addr, peers, local, root_dir).await?;

    // write the PID to the root dir
    let pid = std::process::id();
    let pid_file = running_node.root_dir_path().join("safenode.pid");
    let mut file = File::create(&pid_file).await?;
    file.write_all(pid.to_string().as_bytes()).await?;

    // Channel to receive node ctrl cmds from RPC service (if enabled), and events monitoring task
    let (ctrl_tx, mut ctrl_rx) = mpsc::channel::<NodeCtrl>(5);

    // Monitor `NodeEvents`
    let node_events_rx = running_node.node_events_channel().subscribe();
    monitor_node_events(node_events_rx, ctrl_tx.clone());

    // Start up gRPC interface if enabled by user
    if let Some(addr) = rpc {
        rpc::start_rpc_service(
            addr,
            log_dir,
            running_node.clone(),
            ctrl_tx,
            started_instant,
        );
    }

    // Keep the node and gRPC service (if enabled) running.
    // We'll monitor any NodeCtrl cmd to restart/stop/update,
    loop {
        match ctrl_rx.recv().await {
            Some(NodeCtrl::Restart(delay)) => {
                let msg = format!("Node is wiping data and restarting in {delay:?}...");
                info!("{msg}");
                println!("{msg} Node path: {log_dir}");
                println!("Wiping node root dir: {:?}", running_node.root_dir_path());
                sleep(delay).await;

                // remove the whole node dir
                let _ =
                    tokio::fs::remove_file(running_node.root_dir_path().join("secret-key")).await;
                break Ok(());
            }
            Some(NodeCtrl::Stop { delay, cause }) => {
                let msg = format!("Node is stopping in {delay:?}...");
                info!("{msg}");
                println!("{msg} Node log path: {log_dir}");
                sleep(delay).await;
                return Err(cause);
            }
            Some(NodeCtrl::Update(_delay)) => {
                // TODO: implement self-update once safenode app releases are published again
                println!("No self-update supported yet.");
            }
            None => {
                info!("Internal node ctrl cmds channel has been closed, restarting node");
                break Ok(());
            }
        }
    }
}

fn monitor_node_events(mut node_events_rx: NodeEventsReceiver, ctrl_tx: mpsc::Sender<NodeCtrl>) {
    let _handle = tokio::spawn(async move {
        loop {
            match node_events_rx.recv().await {
                Ok(NodeEvent::ConnectedToNetwork) => Marker::NodeConnectedToNetwork.log(),
                Ok(NodeEvent::ChannelClosed) | Err(RecvError::Closed) => {
                    if let Err(err) = ctrl_tx
                        .send(NodeCtrl::Stop {
                            delay: Duration::from_secs(1),
                            cause: eyre!("Node events channel closed!"),
                        })
                        .await
                    {
                        error!(
                            "Failed to send node control msg to safenode bin main thread: {err}"
                        );
                        break;
                    }
                }
                Ok(NodeEvent::BehindNat) => {
                    if let Err(err) = ctrl_tx
                        .send(NodeCtrl::Stop {
                            delay: Duration::from_secs(1),
                            cause: eyre!("We have been determined to be behind a NAT. This means we are not reachable externally by other nodes. In the future, the network will implement relays that allow us to still join the network."),
                        })
                        .await
                    {
                        error!(
                            "Failed to send node control msg to safenode bin main thread: {err}"
                        );
                        break;
                    }
                }
                Ok(event) => {
                    /* we ignore other events */
                    info!("Currently ignored node event {event:?}");
                }
                Err(RecvError::Lagged(n)) => {
                    warn!("Skipped {n} node events!");
                    continue;
                }
            }
        }
    });
}

async fn create_secret_key_file(path: impl AsRef<Path>) -> Result<tokio::fs::File, std::io::Error> {
    let mut opt = tokio::fs::OpenOptions::new();
    opt.write(true).create_new(true);

    // On Unix systems, make sure only the current user can read/write.
    #[cfg(unix)]
    let opt = opt.mode(0o600);

    opt.open(path).await
}

async fn keypair_from_path(path: impl AsRef<Path>) -> Result<Keypair> {
    let keypair = match std::fs::read(&path) {
        // If the file is opened successfully, read the key from it
        Ok(key) => {
            let keypair = Keypair::ed25519_from_bytes(key)
                .map_err(|err| eyre!("could not read ed25519 key from file: {err}"))?;

            info!("loaded secret key from file: {:?}", path.as_ref());

            keypair
        }
        // In case the file is not found, generate a new keypair and write it to the file
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            let secret_key = libp2p::identity::ed25519::SecretKey::generate();
            let mut file = create_secret_key_file(&path)
                .await
                .map_err(|err| eyre!("could not create secret key file: {err}"))?;
            file.write_all(secret_key.as_ref()).await?;

            info!("generated new key and stored to file: {:?}", path.as_ref());

            libp2p::identity::ed25519::Keypair::from(secret_key).into()
        }
        // Else the file can't be opened, for whatever reason (e.g. permissions).
        Err(err) => {
            return Err(eyre!("failed to read secret key file: {err}"));
        }
    };

    Ok(keypair)
}

fn get_root_dir(peer_id: PeerId) -> Result<PathBuf> {
    let dir = dirs_next::data_dir()
        .ok_or_else(|| eyre!("could not obtain root directory path".to_string()))?
        .join("safe")
        .join("node")
        .join(peer_id.to_string());

    Ok(dir)
}

/// The keypair is located inside the root directory. At the same time, when no dir is specified,
/// the dir name is derived from the keypair used in the application: the peer ID is used as the directory name.
async fn get_root_dir_and_keypair(root_dir: Option<PathBuf>) -> Result<(PathBuf, Keypair)> {
    match root_dir {
        Some(dir) => {
            tokio::fs::create_dir_all(&dir).await?;

            let secret_key_path = dir.join("secret-key");
            Ok((dir, keypair_from_path(secret_key_path).await?))
        }
        None => {
            let secret_key = libp2p::identity::ed25519::SecretKey::generate();
            let keypair: Keypair =
                libp2p::identity::ed25519::Keypair::from(secret_key.clone()).into();
            let peer_id = keypair.public().to_peer_id();

            let dir = get_root_dir(peer_id)?;
            tokio::fs::create_dir_all(&dir).await?;

            let secret_key_path = dir.join("secret-key");

            let mut file = create_secret_key_file(&secret_key_path)
                .await
                .map_err(|err| eyre!("could not create secret key file: {err}"))?;
            file.write_all(secret_key.as_ref()).await?;

            Ok((dir, keypair))
        }
    }
}
