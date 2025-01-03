// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

#[macro_use]
extern crate tracing;

mod rpc_service;
mod subcommands;

use crate::subcommands::EvmNetworkCommand;
use ant_bootstrap::{BootstrapCacheConfig, BootstrapCacheStore, PeersArgs};
use ant_evm::{get_evm_network_from_env, local_evm_network_from_csv, EvmNetwork, RewardsAddress};
use ant_logging::metrics::init_metrics;
use ant_logging::{Level, LogFormat, LogOutputDest, ReloadHandle};
use ant_node::{Marker, NodeBuilder, NodeEvent, NodeEventsReceiver};
use ant_protocol::{
    node::get_antnode_root_dir,
    node_rpc::{NodeCtrl, StopResult},
    version,
};
use clap::{command, Parser};
use color_eyre::{eyre::eyre, Result};
use const_hex::traits::FromHex;
use libp2p::{identity::Keypair, PeerId};
use std::{
    env,
    io::Write,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};
use sysinfo::{self, System};
use tokio::{
    runtime::Runtime,
    sync::{broadcast::error::RecvError, mpsc},
    time::sleep,
};
use tracing_appender::non_blocking::WorkerGuard;

#[derive(Debug, Clone)]
pub enum LogOutputDestArg {
    Stdout,
    DataDir,
    Path(PathBuf),
}

impl std::fmt::Display for LogOutputDestArg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LogOutputDestArg::Stdout => write!(f, "stdout"),
            LogOutputDestArg::DataDir => write!(f, "data-dir"),
            LogOutputDestArg::Path(path) => write!(f, "{}", path.display()),
        }
    }
}

pub fn parse_log_output(val: &str) -> Result<LogOutputDestArg> {
    match val {
        "stdout" => Ok(LogOutputDestArg::Stdout),
        "data-dir" => Ok(LogOutputDestArg::DataDir),
        // The path should be a directory, but we can't use something like `is_dir` to check
        // because the path doesn't need to exist. We can create it for the user.
        value => Ok(LogOutputDestArg::Path(PathBuf::from(value))),
    }
}

// Please do not remove the blank lines in these doc comments.
// They are used for inserting line breaks when the help menu is rendered in the UI.
#[derive(Parser, Debug)]
#[command(disable_version_flag = true)]
#[clap(name = "antnode cli", version = env!("CARGO_PKG_VERSION"))]
struct Opt {
    /// Specify whether the node is operating from a home network and situated behind a NAT without port forwarding
    /// capabilities. Setting this to true, activates hole-punching to facilitate direct connections from other nodes.
    ///
    /// If this not enabled and you're behind a NAT, the node is terminated.
    #[clap(long, default_value_t = false)]
    home_network: bool,

    /// Try to use UPnP to open a port in the home router and allow incoming connections.
    #[cfg(feature = "upnp")]
    #[clap(long, default_value_t = false)]
    upnp: bool,

    /// Specify the logging output destination.
    ///
    /// Valid values are "stdout", "data-dir", or a custom path.
    ///
    /// `data-dir` is the default value.
    ///
    /// The data directory location is platform specific:
    ///  - Linux: $HOME/.local/share/autonomi/node/<peer-id>/logs
    ///  - macOS: $HOME/Library/Application Support/autonomi/node/<peer-id>/logs
    ///  - Windows: C:\Users\<username>\AppData\Roaming\autonomi\node\<peer-id>\logs
    #[expect(rustdoc::invalid_html_tags)]
    #[clap(long, default_value_t = LogOutputDestArg::DataDir, value_parser = parse_log_output, verbatim_doc_comment)]
    log_output_dest: LogOutputDestArg,

    /// Specify the logging format.
    ///
    /// Valid values are "default" or "json".
    ///
    /// If the argument is not used, the default format will be applied.
    #[clap(long, value_parser = LogFormat::parse_from_str, verbatim_doc_comment)]
    log_format: Option<LogFormat>,

    /// Specify the maximum number of uncompressed log files to store.
    ///
    /// This argument is ignored if `log_output_dest` is set to "stdout"
    ///
    /// After reaching this limit, the older files are archived to save space.
    /// You can also specify the maximum number of archived log files to keep.
    #[clap(long, verbatim_doc_comment)]
    max_log_files: Option<usize>,

    /// Specify the maximum number of archived log files to store.
    ///
    /// This argument is ignored if `log_output_dest` is set to "stdout"
    ///
    /// After reaching this limit, the older archived files are deleted.
    #[clap(long, verbatim_doc_comment)]
    max_archived_log_files: Option<usize>,

    /// Specify the network ID to use. This will allow you to run the node on a different network.
    ///
    /// By default, the network ID is set to 1, which represents the mainnet.
    #[clap(long, verbatim_doc_comment)]
    network_id: Option<u8>,

    /// Specify the rewards address.
    /// The rewards address is the address that will receive the rewards for the node.
    /// It should be a valid EVM address.
    #[clap(long)]
    rewards_address: Option<String>,

    /// Specify the EVM network to use.
    /// The network can either be a pre-configured one or a custom network.
    /// When setting a custom network, you must specify the RPC URL to a fully synced node and
    /// the addresses of the network token and chunk payments contracts.
    #[command(subcommand)]
    evm_network: Option<EvmNetworkCommand>,

    /// Specify the node's data directory.
    ///
    /// If not provided, the default location is platform specific:
    ///  - Linux: $HOME/.local/share/autonomi/node/<peer-id>
    ///  - macOS: $HOME/Library/Application Support/autonomi/node/<peer-id>
    ///  - Windows: C:\Users\<username>\AppData\Roaming\autonomi\node\<peer-id>
    #[expect(rustdoc::invalid_html_tags)]
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

    /// Specify the owner(readable discord user name).
    #[clap(long)]
    owner: Option<String>,

    #[cfg(feature = "open-metrics")]
    /// Specify the port for the OpenMetrics server.
    ///
    /// If set, `--enable-metrics-server` will automatically be set to true.
    /// If not set, you must manually specify `--enable-metrics-server` and a port will be selected at random.
    #[clap(long, default_value_t = 0)]
    metrics_server_port: u16,

    #[cfg(feature = "open-metrics")]
    /// Start the metrics server.
    ///
    /// This is automatically enabled if `metrics_server_port` is specified.
    #[clap(
        long,
        default_value_t = false,
        required_if_eq("metrics_server_port", "0")
    )]
    enable_metrics_server: bool,

    /// Print the crate version.
    #[clap(long)]
    crate_version: bool,

    /// Print the network protocol version.
    #[clap(long)]
    protocol_version: bool,

    /// Print the package version.
    #[cfg(not(feature = "nightly"))]
    #[clap(long)]
    package_version: bool,

    /// Print version information.
    #[clap(long)]
    version: bool,
}

fn main() -> Result<()> {
    color_eyre::install()?;
    let opt = Opt::parse();

    if let Some(network_id) = opt.network_id {
        version::set_network_id(network_id);
    }

    let identify_protocol_str = version::IDENTIFY_PROTOCOL_STR
        .read()
        .expect("Failed to obtain read lock for IDENTIFY_PROTOCOL_STR");
    if opt.version {
        println!(
            "{}",
            ant_build_info::version_string(
                "Autonomi Node",
                env!("CARGO_PKG_VERSION"),
                Some(&identify_protocol_str)
            )
        );
        return Ok(());
    }

    // evm config
    let rewards_address = RewardsAddress::from_hex(opt.rewards_address.as_ref().expect(
        "the following required arguments were not provided: --rewards-address <REWARDS_ADDRESS>",
    ))?;

    if opt.crate_version {
        println!("Crate version: {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    if opt.protocol_version {
        println!("Network version: {identify_protocol_str}");
        return Ok(());
    }

    #[cfg(not(feature = "nightly"))]
    if opt.package_version {
        println!("Package version: {}", ant_build_info::package_version());
        return Ok(());
    }

    let evm_network: EvmNetwork = if opt.peers.local {
        println!("Running node in local mode");
        local_evm_network_from_csv()?
    } else {
        opt.evm_network
            .as_ref()
            .cloned()
            .map(|v| Ok(v.into()))
            .unwrap_or_else(get_evm_network_from_env)?
    };
    println!("EVM network: {evm_network:?}");

    let node_socket_addr = SocketAddr::new(opt.ip, opt.port);
    let (root_dir, keypair) = get_root_dir_and_keypair(&opt.root_dir)?;

    let (log_output_dest, log_reload_handle, _log_appender_guard) =
        init_logging(&opt, keypair.public().to_peer_id())?;

    let rt = Runtime::new()?;
    let mut bootstrap_cache = BootstrapCacheStore::new_from_peers_args(
        &opt.peers,
        Some(BootstrapCacheConfig::default_config()?),
    )?;
    // To create the file before startup if it doesn't exist.
    bootstrap_cache.sync_and_flush_to_disk(true)?;

    let msg = format!(
        "Running {} v{}",
        env!("CARGO_BIN_NAME"),
        env!("CARGO_PKG_VERSION")
    );
    info!("\n{}\n{}", msg, "=".repeat(msg.len()));

    ant_build_info::log_version_info(env!("CARGO_PKG_VERSION"), &identify_protocol_str);
    debug!(
        "antnode built with git version: {}",
        ant_build_info::git_info()
    );

    info!(
        "Node started with bootstrap cache containing {} peers",
        bootstrap_cache.peer_count()
    );

    // Create a tokio runtime per `run_node` attempt, this ensures
    // any spawned tasks are closed before we would attempt to run
    // another process with these args.
    if opt.peers.local {
        rt.spawn(init_metrics(std::process::id()));
    }
    let initial_peres = rt.block_on(opt.peers.get_addrs(None, Some(100)))?;
    debug!("Node's owner set to: {:?}", opt.owner);
    let restart_options = rt.block_on(async move {
        let mut node_builder = NodeBuilder::new(
            keypair,
            rewards_address,
            evm_network,
            node_socket_addr,
            opt.peers.local,
            root_dir,
            #[cfg(feature = "upnp")]
            opt.upnp,
        );
        node_builder.initial_peers(initial_peres);
        node_builder.bootstrap_cache(bootstrap_cache);
        node_builder.is_behind_home_network(opt.home_network);
        #[cfg(feature = "open-metrics")]
        let mut node_builder = node_builder;
        // if enable flag is provided or only if the port is specified then enable the server by setting Some()
        #[cfg(feature = "open-metrics")]
        let metrics_server_port = if opt.enable_metrics_server || opt.metrics_server_port != 0 {
            Some(opt.metrics_server_port)
        } else {
            None
        };
        #[cfg(feature = "open-metrics")]
        node_builder.metrics_server_port(metrics_server_port);
        let restart_options =
            run_node(node_builder, opt.rpc, &log_output_dest, log_reload_handle).await?;

        Ok::<_, eyre::Report>(restart_options)
    })?;

    // actively shut down the runtime
    rt.shutdown_timeout(Duration::from_secs(2));

    // Restart only if we received a restart command.
    if let Some((retain_peer_id, root_dir, port)) = restart_options {
        start_new_node_process(retain_peer_id, root_dir, port);
        println!("A new node process has been started successfully.");
    } else {
        println!("The node process has been stopped.");
    }

    Ok(())
}

/// Start a node with the given configuration.
/// Returns:
/// - `Ok(Some(_))` if we receive a restart request.
/// - `Ok(None)` if we want to shutdown the node.
/// - `Err(_)` if we want to shutdown the node with an error.
async fn run_node(
    node_builder: NodeBuilder,
    rpc: Option<SocketAddr>,
    log_output_dest: &str,
    log_reload_handle: ReloadHandle,
) -> Result<Option<(bool, PathBuf, u16)>> {
    let started_instant = std::time::Instant::now();

    info!("Starting node ...");
    let running_node = node_builder.build_and_run()?;

    println!(
        "
Node started

PeerId is {}
You can check your reward balance by running:
`safe wallet balance --peer-id={}`
    ",
        running_node.peer_id(),
        running_node.peer_id()
    );

    // write the PID to the root dir
    let pid = std::process::id();
    let pid_file = running_node.root_dir_path().join("antnode.pid");
    std::fs::write(pid_file, pid.to_string().as_bytes())?;

    // Channel to receive node ctrl cmds from RPC service (if enabled), and events monitoring task
    let (ctrl_tx, mut ctrl_rx) = mpsc::channel::<NodeCtrl>(5);

    // Monitor `NodeEvents`
    let node_events_rx = running_node.node_events_channel().subscribe();
    monitor_node_events(node_events_rx, ctrl_tx.clone());

    // Monitor ctrl-c
    let ctrl_tx_clone = ctrl_tx.clone();
    tokio::spawn(async move {
        if let Err(err) = tokio::signal::ctrl_c().await {
            // I/O error, ignore/print the error, but continue to handle as if ctrl-c was received
            warn!("Listening to ctrl-c error: {err}");
        }
        if let Err(err) = ctrl_tx_clone
            .send(NodeCtrl::Stop {
                delay: Duration::from_secs(1),
                result: StopResult::Error(eyre!("Ctrl-C received!")),
            })
            .await
        {
            error!("Failed to send node control msg to antnode bin main thread: {err}");
        }
    });
    let ctrl_tx_clone_cpu = ctrl_tx.clone();
    // Monitor host CPU usage
    tokio::spawn(async move {
        use rand::{thread_rng, Rng};

        const CPU_CHECK_INTERVAL: Duration = Duration::from_secs(60);
        const CPU_USAGE_THRESHOLD: f32 = 50.0;
        const HIGH_CPU_CONSECUTIVE_LIMIT: u8 = 5;
        const NODE_STOP_DELAY: Duration = Duration::from_secs(1);
        const INITIAL_DELAY_MIN_S: u64 = 10;
        const INITIAL_DELAY_MAX_S: u64 =
            HIGH_CPU_CONSECUTIVE_LIMIT as u64 * CPU_CHECK_INTERVAL.as_secs();
        const JITTER_MIN_S: u64 = 1;
        const JITTER_MAX_S: u64 = 15;

        let mut sys = System::new_all();

        let mut high_cpu_count: u8 = 0;

        // Random initial delay between 1 and 5 minutes
        let initial_delay =
            Duration::from_secs(thread_rng().gen_range(INITIAL_DELAY_MIN_S..=INITIAL_DELAY_MAX_S));
        tokio::time::sleep(initial_delay).await;

        loop {
            sys.refresh_cpu();
            let cpu_usage = sys.global_cpu_info().cpu_usage();

            if cpu_usage > CPU_USAGE_THRESHOLD {
                high_cpu_count += 1;
            } else {
                high_cpu_count = 0;
            }

            if high_cpu_count >= HIGH_CPU_CONSECUTIVE_LIMIT {
                if let Err(err) = ctrl_tx_clone_cpu
                    .send(NodeCtrl::Stop {
                        delay: NODE_STOP_DELAY,
                        result: StopResult::Success(format!("Excess host CPU %{CPU_USAGE_THRESHOLD} detected for {HIGH_CPU_CONSECUTIVE_LIMIT} consecutive minutes!")),
                    })
                    .await
                {
                    error!("Failed to send node control msg to antnode bin main thread: {err}");
                }
                break;
            }

            // Add jitter to the interval
            let jitter = Duration::from_secs(thread_rng().gen_range(JITTER_MIN_S..=JITTER_MAX_S));
            tokio::time::sleep(CPU_CHECK_INTERVAL + jitter).await;
        }
    });

    // Start up gRPC interface if enabled by user
    if let Some(addr) = rpc {
        rpc_service::start_rpc_service(
            addr,
            log_output_dest,
            running_node.clone(),
            ctrl_tx,
            started_instant,
            log_reload_handle,
        );
    }

    // Keep the node and gRPC service (if enabled) running.
    // We'll monitor any NodeCtrl cmd to restart/stop/update,
    loop {
        match ctrl_rx.recv().await {
            Some(NodeCtrl::Restart {
                delay,
                retain_peer_id,
            }) => {
                let root_dir = running_node.root_dir_path();
                let node_port = running_node.get_node_listening_port().await?;

                let msg = format!("Node is restarting in {delay:?}...");
                info!("{msg}");
                println!("{msg} Node path: {log_output_dest}");
                sleep(delay).await;

                return Ok(Some((retain_peer_id, root_dir, node_port)));
            }
            Some(NodeCtrl::Stop { delay, result }) => {
                let msg = format!("Node is stopping in {delay:?}...");
                info!("{msg}");
                println!("{msg} Node log path: {log_output_dest}");
                sleep(delay).await;
                match result {
                    StopResult::Success(message) => {
                        info!("Node stopped successfully: {}", message);
                        return Ok(None);
                    }
                    StopResult::Error(cause) => {
                        error!("Node stopped with error: {}", cause);
                        return Err(cause);
                    }
                }
            }
            Some(NodeCtrl::Update(_delay)) => {
                // TODO: implement self-update once antnode app releases are published again
                println!("No self-update supported yet.");
            }
            None => {
                info!("Internal node ctrl cmds channel has been closed, restarting node");
                break Err(eyre!("Internal node ctrl cmds channel has been closed"));
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
                            result: StopResult::Error(eyre!("Node events channel closed!")),
                        })
                        .await
                    {
                        error!("Failed to send node control msg to antnode bin main thread: {err}");
                        break;
                    }
                }
                Ok(NodeEvent::TerminateNode(reason)) => {
                    if let Err(err) = ctrl_tx
                        .send(NodeCtrl::Stop {
                            delay: Duration::from_secs(1),
                            result: StopResult::Error(eyre!("Node terminated due to: {reason:?}")),
                        })
                        .await
                    {
                        error!("Failed to send node control msg to antnode bin main thread: {err}");
                        break;
                    }
                }
                Ok(event) => {
                    /* we ignore other events */
                    debug!("Currently ignored node event {event:?}");
                }
                Err(RecvError::Lagged(n)) => {
                    warn!("Skipped {n} node events!");
                    continue;
                }
            }
        }
    });
}

fn init_logging(opt: &Opt, peer_id: PeerId) -> Result<(String, ReloadHandle, Option<WorkerGuard>)> {
    let logging_targets = vec![
        ("ant_bootstrap".to_string(), Level::INFO),
        ("ant_build_info".to_string(), Level::DEBUG),
        ("ant_evm".to_string(), Level::DEBUG),
        ("ant_logging".to_string(), Level::DEBUG),
        ("ant_networking".to_string(), Level::INFO),
        ("ant_node".to_string(), Level::DEBUG),
        ("ant_protocol".to_string(), Level::DEBUG),
        ("ant_registers".to_string(), Level::DEBUG),
        ("antnode".to_string(), Level::DEBUG),
    ];

    let output_dest = match &opt.log_output_dest {
        LogOutputDestArg::Stdout => LogOutputDest::Stdout,
        LogOutputDestArg::DataDir => {
            let path = get_antnode_root_dir(peer_id)?.join("logs");
            LogOutputDest::Path(path)
        }
        LogOutputDestArg::Path(path) => LogOutputDest::Path(path.clone()),
    };

    #[cfg(not(feature = "otlp"))]
    let (reload_handle, log_appender_guard) = {
        let mut log_builder = ant_logging::LogBuilder::new(logging_targets);
        log_builder.output_dest(output_dest.clone());
        log_builder.format(opt.log_format.unwrap_or(LogFormat::Default));
        if let Some(files) = opt.max_log_files {
            log_builder.max_log_files(files);
        }
        if let Some(files) = opt.max_archived_log_files {
            log_builder.max_archived_log_files(files);
        }

        log_builder.initialize()?
    };

    #[cfg(feature = "otlp")]
    let (_rt, reload_handle, log_appender_guard) = {
        // init logging in a separate runtime if we are sending traces to an opentelemetry server
        let rt = Runtime::new()?;
        let (reload_handle, log_appender_guard) = rt.block_on(async {
            let mut log_builder = ant_logging::LogBuilder::new(logging_targets);
            log_builder.output_dest(output_dest.clone());
            log_builder.format(opt.log_format.unwrap_or(LogFormat::Default));
            if let Some(files) = opt.max_log_files {
                log_builder.max_log_files(files);
            }
            if let Some(files) = opt.max_archived_log_files {
                log_builder.max_archived_log_files(files);
            }
            log_builder.initialize()
        })?;
        (rt, reload_handle, log_appender_guard)
    };

    Ok((output_dest.to_string(), reload_handle, log_appender_guard))
}

fn create_secret_key_file(path: impl AsRef<Path>) -> Result<std::fs::File, std::io::Error> {
    let mut opt = std::fs::OpenOptions::new();
    opt.write(true).create_new(true);

    // On Unix systems, make sure only the current user can read/write.
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opt.mode(0o600);
    }

    opt.open(path)
}

fn keypair_from_path(path: impl AsRef<Path>) -> Result<Keypair> {
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
                .map_err(|err| eyre!("could not create secret key file: {err}"))?;
            file.write_all(secret_key.as_ref())?;

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

/// The keypair is located inside the root directory. At the same time, when no dir is specified,
/// the dir name is derived from the keypair used in the application: the peer ID is used as the directory name.
fn get_root_dir_and_keypair(root_dir: &Option<PathBuf>) -> Result<(PathBuf, Keypair)> {
    match root_dir {
        Some(dir) => {
            std::fs::create_dir_all(dir)?;

            let secret_key_path = dir.join("secret-key");
            Ok((dir.clone(), keypair_from_path(secret_key_path)?))
        }
        None => {
            let secret_key = libp2p::identity::ed25519::SecretKey::generate();
            let keypair: Keypair =
                libp2p::identity::ed25519::Keypair::from(secret_key.clone()).into();
            let peer_id = keypair.public().to_peer_id();

            let dir = get_antnode_root_dir(peer_id)?;
            std::fs::create_dir_all(&dir)?;

            let secret_key_path = dir.join("secret-key");

            let mut file = create_secret_key_file(secret_key_path)
                .map_err(|err| eyre!("could not create secret key file: {err}"))?;
            file.write_all(secret_key.as_ref())?;

            Ok((dir, keypair))
        }
    }
}

/// Starts a new process running the binary with the same args as
/// the current process
/// Optionally provide the node's root dir and listen port to retain it's PeerId
fn start_new_node_process(retain_peer_id: bool, root_dir: PathBuf, port: u16) {
    // Retrieve the current executable's path
    let current_exe = env::current_exe().expect("could not get current executable path");

    // Retrieve the command-line arguments passed to this process
    let args: Vec<String> = env::args().collect();

    info!("Original args are: {args:?}");
    info!("Current exe is: {current_exe:?}");

    // Convert current exe path to string, log an error and return if it fails
    let current_exe = match current_exe.to_str() {
        Some(s) => {
            // remove "(deleted)" string from current exe path
            if s.contains(" (deleted)") {
                warn!("The current executable path contains ' (deleted)', which may lead to unexpected behavior. This has been removed from the exe location string");
                s.replace(" (deleted)", "")
            } else {
                s.to_string()
            }
        }
        None => {
            error!("Failed to convert current executable path to string");
            return;
        }
    };

    // Create a new Command instance to run the current executable
    let mut cmd = Command::new(current_exe);

    // Set the arguments for the new Command
    cmd.args(&args[1..]); // Exclude the first argument (binary path)

    if retain_peer_id {
        cmd.arg("--root-dir");
        cmd.arg(format!("{root_dir:?}"));
        cmd.arg("--port");
        cmd.arg(port.to_string());
    }

    warn!(
        "Attempting to start a new process as node process loop has been broken: {:?}",
        cmd
    );
    // Execute the command
    let _handle = match cmd.spawn() {
        Ok(status) => status,
        Err(e) => {
            // Do not return an error as this isn't a critical failure.
            // The current node can continue.
            eprintln!("Failed to execute hard-restart command: {e:?}");
            error!("Failed to execute hard-restart command: {e:?}");

            return;
        }
    };
}
