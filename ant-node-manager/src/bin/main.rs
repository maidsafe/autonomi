// Copyright (C) 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use ant_bootstrap::InitialPeersConfig;
use ant_evm::RewardsAddress;
use ant_evm::{EvmNetwork, get_evm_network};
use ant_logging::{LogBuilder, LogFormat};
use ant_node_manager::{
    DEFAULT_NODE_STARTUP_INTERVAL_MS, VerbosityLevel,
    add_services::config::PortRange,
    cmd::{self},
    config,
};
use ant_service_management::NodeRegistryManager;
use clap::Parser;
use clap::Subcommand;
use color_eyre::Result;
use color_eyre::eyre::eyre;
use std::{net::Ipv4Addr, path::PathBuf};
use tracing::Level;

const DEFAULT_NODE_COUNT: u16 = 25;

#[derive(Parser)]
#[command(disable_version_flag = true)]
pub(crate) struct Cmd {
    /// Available sub commands.
    #[clap(subcommand)]
    pub cmd: Option<SubCmd>,

    /// Print the crate version.
    #[clap(long)]
    pub crate_version: bool,

    /// Output debug-level logging to stderr.
    #[clap(long, conflicts_with = "trace")]
    debug: bool,

    /// Print the package version.
    #[cfg(not(feature = "nightly"))]
    #[clap(long)]
    pub package_version: bool,

    /// Output trace-level logging to stderr.
    #[clap(long, conflicts_with = "debug")]
    trace: bool,

    #[clap(short, long, action = clap::ArgAction::Count, default_value_t = 2)]
    verbose: u8,

    /// Print version information.
    #[clap(long)]
    version: bool,
}

#[derive(Subcommand, Clone, Debug)]
#[allow(clippy::enum_variant_names)]
pub enum EvmNetworkCommand {
    /// Use the Arbitrum One network
    EvmArbitrumOne,

    /// Use the Arbitrum Sepolia network with test contracts
    EvmArbitrumSepoliaTest,

    /// Use a custom network
    EvmCustom {
        /// The RPC URL for the custom network
        #[arg(long)]
        rpc_url: String,

        /// The payment token contract address
        #[arg(long, short)]
        payment_token_address: String,

        /// The chunk payments contract address
        #[arg(long, short)]
        data_payments_address: String,
    },

    /// Use the local EVM testnet, loaded from a CSV file.
    EvmLocal,
}

impl TryInto<EvmNetwork> for EvmNetworkCommand {
    type Error = color_eyre::eyre::Error;

    fn try_into(self) -> Result<EvmNetwork> {
        match self {
            Self::EvmArbitrumOne => Ok(EvmNetwork::ArbitrumOne),
            Self::EvmArbitrumSepoliaTest => Ok(EvmNetwork::ArbitrumSepoliaTest),
            Self::EvmLocal => {
                let network = get_evm_network(true, None)?;
                Ok(network)
            }
            Self::EvmCustom {
                rpc_url,
                payment_token_address,
                data_payments_address,
            } => Ok(EvmNetwork::new_custom(
                &rpc_url,
                &payment_token_address,
                &data_payments_address,
            )),
        }
    }
}

#[derive(Subcommand, Debug)]
pub enum SubCmd {
    /// Add one or more antnode services.
    ///
    /// By default, the latest antnode binary will be downloaded; however, it is possible to
    /// provide a binary either by specifying a URL, a local path, or a specific version number.
    ///
    /// On Windows, this command must run with administrative privileges.
    ///
    /// On macOS and most distributions of Linux, the command does not require elevated privileges,
    /// but it *can* be used with sudo if desired. If the command runs without sudo, services will
    /// be defined as user-mode services; otherwise, they will be created as system-wide services.
    /// The main difference is that user-mode services require an active user session, whereas a
    /// system-wide service can run completely in the background, without any user session.
    ///
    /// On some distributions of Linux, e.g., Alpine, sudo will be required. This is because the
    /// OpenRC service manager, which is used on Alpine, doesn't support user-mode services. Most
    /// distributions, however, use Systemd, which *does* support user-mode services.
    #[clap(name = "add")]
    Add {
        /// Set if you want the service to connect to the alpha network.
        #[clap(long, default_value_t = false)]
        alpha: bool,
        /// Set to automatically restart antnode services upon OS reboot.
        ///
        /// If not used, any added services will *not* restart automatically when the OS reboots
        /// and they will need to be explicitly started again.
        #[clap(long, default_value_t = false)]
        auto_restart: bool,
        /// The number of service instances.
        ///
        /// If the --first argument is used, the count has to be one, so --count and --first are
        /// mutually exclusive.
        #[clap(long, conflicts_with = "first")]
        count: Option<u16>,
        /// Provide the path for the data directory for the installed node.
        ///
        /// This path is a prefix. Each installed node will have its own directory underneath it.
        ///
        /// If not provided, the default location is platform specific:
        ///  - Linux/macOS (system-wide): /var/antctl/services
        ///  - Linux/macOS (user-mode): ~/.local/share/autonomi/node
        ///  - Windows: C:\ProgramData\antnode\services
        #[clap(long, verbatim_doc_comment)]
        data_dir_path: Option<PathBuf>,
        /// Provide environment variables for the antnode service.
        ///
        /// Useful to set log levels. Variables should be comma separated without spaces.
        ///
        /// Example: --env ANT_LOG=all,RUST_LOG=libp2p=debug
        #[clap(name = "env", long, use_value_delimiter = false, value_parser = parse_environment_variables)]
        env_variables: Option<Vec<(String, String)>>,
        /// Specify what EVM network to use for payments.
        #[command(subcommand)]
        evm_network: EvmNetworkCommand,
        /// Provide the path for the log directory for the installed node.
        ///
        /// This path is a prefix. Each installed node will have its own directory underneath it.
        ///
        /// If not provided, the default location is platform specific:
        ///  - Linux/macOS (system-wide): /var/log/antnode
        ///  - Linux/macOS (user-mode): ~/.local/share/autonomi/node/*/logs
        ///  - Windows: C:\ProgramData\antnode\logs
        #[clap(long, verbatim_doc_comment)]
        log_dir_path: Option<PathBuf>,
        /// Specify the logging format for started nodes.
        ///
        /// Valid values are "default" or "json".
        ///
        /// If the argument is not used, the default format will be applied.
        #[clap(long, value_parser = LogFormat::parse_from_str, verbatim_doc_comment)]
        log_format: Option<LogFormat>,
        /// Specify the maximum number of uncompressed log files to store.
        ///
        /// After reaching this limit, the older files are archived to save space.
        /// You can also specify the maximum number of archived log files to keep.
        #[clap(long, verbatim_doc_comment)]
        max_log_files: Option<usize>,
        /// Specify the maximum number of archived log files to store.
        ///
        /// After reaching this limit, the older archived files are deleted.
        #[clap(long, verbatim_doc_comment)]
        max_archived_log_files: Option<usize>,
        /// Specify a port for the open metrics server.
        ///
        /// If you're passing the compiled antnode via --node-path, make sure to enable the open-metrics feature
        /// when compiling.
        ///
        /// If not set, metrics server will be started on a random port.
        ///
        /// If multiple services are being added and this argument is used, you must specify a
        /// range. For example, '12000-12004'. The length of the range must match the number of
        /// services, which in this case would be 5. The range must also go from lower to higher.
        #[clap(long, value_parser = PortRange::parse)]
        metrics_port: Option<PortRange>,
        /// Specify the network ID to use for the services. This will allow you to run the node on a different network.
        ///
        /// By default, the network ID is set to 1, which represents the mainnet.
        #[clap(long, verbatim_doc_comment)]
        network_id: Option<u8>,
        /// Specify the IP address for the antnode service(s).
        ///
        /// If not set, we bind to all the available network interfaces.
        #[clap(long)]
        node_ip: Option<Ipv4Addr>,
        /// Specify a port for the antnode service(s).
        ///
        /// If not used, ports will be selected at random.
        ///
        /// If multiple services are being added and this argument is used, you must specify a
        /// range. For example, '12000-12004'. The length of the range must match the number of
        /// services, which in this case would be 5. The range must also go from lower to higher.
        #[clap(long, value_parser = PortRange::parse)]
        node_port: Option<PortRange>,
        /// Provide a path for the antnode binary to be used by the service.
        ///
        /// Useful for creating the service using a custom built binary.
        #[clap(long)]
        path: Option<PathBuf>,
        #[command(flatten)]
        peers: InitialPeersConfig,
        /// Specify the wallet address that will receive the node's earnings.
        #[clap(long)]
        rewards_address: RewardsAddress,
        /// Specify an Ipv4Addr for the node's RPC server to run on.
        ///
        /// Useful if you want to expose the RPC server pubilcly. Ports are assigned automatically.
        ///
        /// If not set, the RPC server is run locally.
        #[clap(long)]
        rpc_address: Option<Ipv4Addr>,
        /// Specify a port for the RPC service(s).
        ///
        /// If not used, ports will be selected at random.
        ///
        /// If multiple services are being added and this argument is used, you must specify a
        /// range. For example, '12000-12004'. The length of the range must match the number of
        /// services, which in this case would be 5. The range must also go from lower to higher.
        #[clap(long, value_parser = PortRange::parse)]
        rpc_port: Option<PortRange>,
        /// Disable running reachability checks before starting the node.
        ///
        /// Reachability check determines the network connectivity and auto configures the node for you. Disable only
        /// if you are sure about the network configuration.
        #[clap(long, default_value_t = false)]
        skip_reachability_check: bool,
        /// Disables UPnP.
        ///
        /// By default, antnode will try to use UPnP if available. Use this flag to disable UPnP.
        #[clap(long, default_value_t = false)]
        no_upnp: bool,
        /// Provide a antnode binary using a URL.
        ///
        /// The binary must be inside a zip or gzipped tar archive.
        ///
        /// This option can be used to test a antnode binary that has been built from a forked
        /// branch and uploaded somewhere. A typical use case would be for a developer who launches
        /// a testnet to test some changes they have on a fork.
        #[clap(long, conflicts_with = "version")]
        url: Option<String>,
        /// The user the service should run as.
        ///
        /// If the account does not exist, it will be created.
        ///
        /// On Windows this argument will have no effect.
        #[clap(long)]
        user: Option<String>,
        /// Provide a specific version of antnode to be installed.
        ///
        /// The version number should be in the form X.Y.Z, with no 'v' prefix.
        ///
        /// The binary will be downloaded.
        #[clap(long)]
        version: Option<String>,
        /// Set this to true if you want the node to write the cache files in the older formats.
        #[clap(long, default_value_t = false)]
        write_older_cache_files: bool,
    },
    /// Get node reward balances.
    #[clap(name = "balance")]
    Balance {
        /// Display the balance for a specific service using its peer ID.
        ///
        /// The argument can be used multiple times.
        #[clap(long)]
        peer_id: Vec<String>,
        /// Display the balance for a specific service using its name.
        ///
        /// The argument can be used multiple times.
        #[clap(long, conflicts_with = "peer_id")]
        service_name: Vec<String>,
    },
    #[clap(subcommand)]
    Local(LocalSubCmd),
    /// Remove antnode service(s).
    ///
    /// If no peer ID(s) or service name(s) are supplied, all services will be removed.
    ///
    /// Services must be stopped before they can be removed.
    ///
    /// On Windows, this command must run as the administrative user. On Linux/macOS, run using
    /// sudo if you defined system-wide services; otherwise, do not run the command elevated.
    #[clap(name = "remove")]
    Remove {
        /// The peer ID of the service to remove.
        ///
        /// The argument can be used multiple times to remove many services.
        #[clap(long)]
        peer_id: Vec<String>,
        /// The name of the service to remove.
        ///
        /// The argument can be used multiple times to remove many services.
        #[clap(long, conflicts_with = "peer_id")]
        service_name: Vec<String>,
        /// Set this flag to keep the node's data and log directories.
        #[clap(long)]
        keep_directories: bool,
    },
    /// Reset back to a clean base state.
    ///
    /// Stop and remove all services and delete the node registry, which will set the service
    /// counter back to zero.
    ///
    /// This command must run as the root/administrative user.
    #[clap(name = "reset")]
    Reset {
        /// Set to suppress the confirmation prompt.
        #[clap(long, short)]
        force: bool,
    },
    /// Start antnode service(s).
    ///
    /// By default, each node service is started after the previous node has successfully connected to the network or
    /// after the 'connection-timeout' period has been reached for that node. The timeout is 300 seconds by default.
    /// The above behaviour can be overridden by setting a fixed interval between starting each node service using the
    /// 'interval' argument.
    ///
    /// If no peer ID(s) or service name(s) are supplied, all services will be started.
    ///
    /// On Windows, this command must run as the administrative user. On Linux/macOS, run using
    /// sudo if you defined system-wide services; otherwise, do not run the command elevated.
    #[clap(name = "start")]
    Start {
        /// An interval applied between launching each service.
        ///
        /// Units are milliseconds. Defaults to 10s.
        #[clap(long, default_value_t = DEFAULT_NODE_STARTUP_INTERVAL_MS)]
        interval: u64,
        /// The peer ID of the service to start.
        ///
        /// The argument can be used multiple times to start many services.
        #[clap(long)]
        peer_id: Vec<String>,
        /// The name of the service to start.
        ///
        /// The argument can be used multiple times to start many services.
        #[clap(long, conflicts_with = "peer_id")]
        service_name: Vec<String>,
    },
    /// Get the status of services.
    #[clap(name = "status")]
    Status {
        /// Set this flag to display more details
        #[clap(long)]
        details: bool,
        /// Set this flag to return an error if any nodes are not running
        #[clap(long)]
        fail: bool,
        /// Set this flag to output the status as a JSON document
        #[clap(long, conflicts_with = "details")]
        json: bool,
    },
    /// Stop antnode service(s).
    ///
    /// If no peer ID(s) or service name(s) are supplied, all services will be stopped.
    ///
    /// On Windows, this command must run as the administrative user. On Linux/macOS, run using
    /// sudo if you defined system-wide services; otherwise, do not run the command elevated.
    #[clap(name = "stop")]
    Stop {
        /// An interval applied between stopping each service.
        ///
        /// Units are milliseconds.
        #[clap(long)]
        interval: Option<u64>,
        /// The peer ID of the service to stop.
        ///
        /// The argument can be used multiple times to stop many services.
        #[clap(long)]
        peer_id: Vec<String>,
        /// The name of the service to stop.
        ///
        /// The argument can be used multiple times to stop many services.
        #[clap(long, conflicts_with = "peer_id")]
        service_name: Vec<String>,
    },
    /// Upgrade antnode services.
    ///
    /// By default, each node service is started after the previous node has successfully connected to the network or
    /// after the 'connection-timeout' period has been reached for that node. The timeout is 300 seconds by default.
    /// The above behaviour can be overridden by setting a fixed interval between starting each node service using the
    /// 'interval' argument.
    ///
    /// If no peer ID(s) or service name(s) are supplied, all services will be upgraded.
    ///
    /// On Windows, this command must run as the administrative user. On Linux/macOS, run using
    /// sudo if you defined system-wide services; otherwise, do not run the command elevated.
    #[clap(name = "upgrade")]
    Upgrade {
        /// Set this flag to upgrade the nodes without automatically starting them.
        ///
        /// Can be useful for testing scenarios.
        #[clap(long)]
        do_not_start: bool,
        /// Provide environment variables for the antnode service.
        ///
        /// Values set when the service was added will be overridden.
        ///
        /// Useful to set antnode's log levels. Variables should be comma separated without
        /// spaces.
        ///
        /// Example: --env ANT_LOG=all,RUST_LOG=libp2p=debug
        #[clap(name = "env", long, use_value_delimiter = false, value_parser = parse_environment_variables)]
        env_variables: Option<Vec<(String, String)>>,
        /// Set this flag to force the upgrade command to replace binaries without comparing any
        /// version numbers.
        ///
        /// Required if we want to downgrade, or for testing purposes.
        #[clap(long)]
        force: bool,
        /// An interval applied between upgrading each service.
        ///
        /// Units are milliseconds. Defaults to 10s.
        #[clap(long, default_value_t = DEFAULT_NODE_STARTUP_INTERVAL_MS)]
        interval: u64,
        /// Provide a path for the antnode binary to be used by the service.
        ///
        /// Useful for upgrading the service using a custom built binary.
        #[clap(long)]
        path: Option<PathBuf>,
        /// The peer ID of the service to upgrade
        #[clap(long)]
        peer_id: Vec<String>,
        /// The name of the service to upgrade
        #[clap(long, conflicts_with = "peer_id")]
        service_name: Vec<String>,
        /// Provide a binary to upgrade to using a URL.
        ///
        /// The binary must be inside a zip or gzipped tar archive.
        ///
        /// This can be useful for testing scenarios.
        #[clap(long, conflicts_with = "version")]
        url: Option<String>,
        /// Upgrade to a specific version rather than the latest version.
        ///
        /// The version number should be in the form X.Y.Z, with no 'v' prefix.
        #[clap(long)]
        version: Option<String>,
    },
}

/// Manage local networks.
#[derive(Subcommand, Debug)]
pub enum LocalSubCmd {
    /// Kill the running local network.
    #[clap(name = "kill")]
    Kill {
        /// Set this flag to keep the node's data and log directories.
        #[clap(long)]
        keep_directories: bool,
    },
    /// Join an existing local network.
    ///
    /// The existing network can be managed outwith the node manager. If this is the case, use the
    /// `--peer` argument to specify an initial peer to connect to.
    ///
    /// If no `--peer` argument is supplied, the nodes will be added to the existing local network
    /// being managed by the node manager.
    #[clap(name = "join")]
    Join {
        /// Set to build the antnode and faucet binaries.
        ///
        /// This option requires the command run from the root of the autonomi repository.
        #[clap(long)]
        build: bool,
        /// The number of nodes to run.
        #[clap(long, default_value_t = DEFAULT_NODE_COUNT)]
        count: u16,
        /// Set this flag to enable the metrics server. The ports will be selected at random.
        ///
        /// If you're passing the compiled antnode via --node-path, make sure to enable the open-metrics feature flag
        /// on the antnode when compiling. If you're using --build, then make sure to enable the feature flag on
        /// antctl.
        ///
        /// An interval applied between launching each node.
        ///
        /// Units are milliseconds.
        #[clap(long, default_value_t = 200)]
        interval: u64,
        /// Specify the logging format.
        ///
        /// Valid values are "default" or "json".
        ///
        /// If the argument is not used, the default format will be applied.
        #[clap(long, value_parser = LogFormat::parse_from_str, verbatim_doc_comment)]
        log_format: Option<LogFormat>,
        /// Specify a port for the open metrics server.
        ///
        /// If you're passing the compiled antnode via --node-path, make sure to enable the open-metrics feature flag
        /// on the antnode when compiling. If you're using --build, then make sure to enable the feature flag on
        /// antctl.
        ///
        /// If not set, metrics server will not be started. Use --enable-metrics-server to start
        /// the metrics server without specifying a port.
        ///
        /// If multiple services are being added and this argument is used, you must specify a
        /// range. For example, '12000-12004'. The length of the range must match the number of
        /// services, which in this case would be 5. The range must also go from lower to higher.
        #[clap(long, value_parser = PortRange::parse)]
        metrics_port: Option<PortRange>,
        /// Path to a antnode binary.
        ///
        /// Make sure to enable the local feature flag on the antnode when compiling the binary.
        ///
        /// The path and version arguments are mutually exclusive.
        #[clap(long, conflicts_with = "node_version")]
        node_path: Option<PathBuf>,
        /// Specify a port for the antnode service(s).
        ///
        /// If not used, ports will be selected at random.
        ///
        /// If multiple services are being added and this argument is used, you must specify a
        /// range. For example, '12000-12004'. The length of the range must match the number of
        /// services, which in this case would be 5. The range must also go from lower to higher.
        #[clap(long, value_parser = PortRange::parse)]
        node_port: Option<PortRange>,
        /// The version of antnode to use.
        ///
        /// The version number should be in the form X.Y.Z, with no 'v' prefix.
        ///
        /// The version and path arguments are mutually exclusive.
        #[clap(long)]
        node_version: Option<String>,
        #[command(flatten)]
        peers: InitialPeersConfig,
        /// Specify a port for the RPC service(s).
        ///
        /// If not used, ports will be selected at random.
        ///
        /// If multiple services are being added and this argument is used, you must specify a
        /// range. For example, '12000-12004'. The length of the range must match the number of
        /// services, which in this case would be 5. The range must also go from lower to higher.
        #[clap(long, value_parser = PortRange::parse)]
        rpc_port: Option<PortRange>,
        /// Specify the wallet address that will receive the node's earnings.
        #[clap(long)]
        rewards_address: RewardsAddress,
        /// Optionally specify what EVM network to use for payments.
        #[command(subcommand)]
        evm_network: Option<EvmNetworkCommand>,
        /// Set to skip the network validation process
        #[clap(long)]
        skip_validation: bool,
    },
    /// Run a local network.
    ///
    /// This will run antnode processes on the current machine to form a local network. A faucet
    /// service will also run for dispensing tokens.
    ///
    /// Paths can be supplied for antnode and faucet binaries, but otherwise, the latest versions
    /// will be downloaded.
    #[clap(name = "run")]
    Run {
        /// Set to build the antnode and faucet binaries.
        ///
        /// This option requires the command run from the root of the autonomi repository.
        #[clap(long)]
        build: bool,
        /// Set to remove the client data directory and kill any existing local network.
        #[clap(long)]
        clean: bool,
        /// The number of nodes to run.
        #[clap(long, default_value_t = DEFAULT_NODE_COUNT)]
        count: u16,
        /// Set this flag to enable the metrics server. The ports will be selected at random.
        ///
        /// If you're passing the compiled antnode via --node-path, make sure to enable the open-metrics feature flag
        /// on the antnode when compiling. If you're using --build, then make sure to enable the feature flag on
        /// antctl.
        ///
        /// An interval applied between launching each node.
        ///
        /// Units are milliseconds.
        #[clap(long, default_value_t = 200)]
        interval: u64,
        /// Specify the logging format.
        ///
        /// Valid values are "default" or "json".
        ///
        /// If the argument is not used, the default format will be applied.
        #[clap(long, value_parser = LogFormat::parse_from_str, verbatim_doc_comment)]
        log_format: Option<LogFormat>,
        /// Specify a port for the open metrics server.
        ///
        /// If you're passing the compiled antnode via --node-path, make sure to enable the open-metrics feature flag
        /// on the antnode when compiling. If you're using --build, then make sure to enable the feature flag on
        /// antctl.
        ///
        /// If not set, metrics server will not be started. Use --enable-metrics-server to start
        /// the metrics server without specifying a port.
        ///
        /// If multiple services are being added and this argument is used, you must specify a
        /// range. For example, '12000-12004'. The length of the range must match the number of
        /// services, which in this case would be 5. The range must also go from lower to higher.
        #[clap(long, value_parser = PortRange::parse)]
        metrics_port: Option<PortRange>,
        /// Path to an antnode binary
        ///
        /// Make sure to enable the local feature flag on the antnode when compiling the binary.
        ///
        /// The path and version arguments are mutually exclusive.
        #[clap(long, conflicts_with = "node_version", conflicts_with = "build")]
        node_path: Option<PathBuf>,
        /// Specify a port for the antnode service(s).
        ///
        /// If not used, ports will be selected at random.
        ///
        /// If multiple services are being added and this argument is used, you must specify a
        /// range. For example, '12000-12004'. The length of the range must match the number of
        /// services, which in this case would be 5. The range must also go from lower to higher.
        #[clap(long, value_parser = PortRange::parse)]
        node_port: Option<PortRange>,
        /// The version of antnode to use.
        ///
        /// The version number should be in the form X.Y.Z, with no 'v' prefix.
        ///
        /// The version and path arguments are mutually exclusive.
        #[clap(long, conflicts_with = "build")]
        node_version: Option<String>,
        /// Specify a port for the RPC service(s).
        ///
        /// If not used, ports will be selected at random.
        ///
        /// If multiple services are being added and this argument is used, you must specify a
        /// range. For example, '12000-12004'. The length of the range must match the number of
        /// services, which in this case would be 5. The range must also go from lower to higher.
        #[clap(long, value_parser = PortRange::parse)]
        rpc_port: Option<PortRange>,
        /// Specify the wallet address that will receive the node's earnings.
        #[clap(long)]
        rewards_address: RewardsAddress,
        /// Optionally specify what EVM network to use for payments.
        #[command(subcommand)]
        evm_network: Option<EvmNetworkCommand>,
        /// Set to skip the network validation process
        #[clap(long)]
        skip_validation: bool,
    },
    /// Get the status of the local nodes.
    #[clap(name = "status")]
    Status {
        /// Set this flag to display more details
        #[clap(long)]
        details: bool,
        /// Set this flag to return an error if any nodes are not running
        #[clap(long)]
        fail: bool,
        /// Set this flag to output the status as a JSON document
        #[clap(long, conflicts_with = "details")]
        json: bool,
    },
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    color_eyre::install()?;
    let args = Cmd::parse();

    if args.version {
        println!(
            "{}",
            ant_build_info::version_string(
                "Autonomi Node Manager",
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

    let verbosity = VerbosityLevel::from(args.verbose);

    let _log_handle = if args.debug || args.trace {
        let level = if args.debug {
            Level::DEBUG
        } else {
            Level::TRACE
        };
        get_log_builder(level)?.initialize()?.1
    } else {
        None
    };

    configure_winsw(verbosity).await?;

    tracing::info!("Executing cmd: {:?}", args.cmd);

    let node_registry = NodeRegistryManager::load(&config::get_node_registry_path()?).await?;
    match args.cmd {
        Some(SubCmd::Add {
            alpha,
            auto_restart,
            count,
            data_dir_path,
            env_variables,
            evm_network,
            log_dir_path,
            log_format,
            max_archived_log_files,
            max_log_files,
            metrics_port,
            network_id,
            node_ip,
            node_port,
            path,
            peers,
            rewards_address,
            rpc_address,
            rpc_port,
            skip_reachability_check,
            url,
            no_upnp,
            user,
            version,
            write_older_cache_files,
        }) => {
            cmd::node::add(
                alpha,
                auto_restart,
                count,
                data_dir_path,
                env_variables,
                Some(evm_network.try_into()?),
                log_dir_path,
                log_format,
                max_archived_log_files,
                max_log_files,
                metrics_port,
                network_id,
                node_ip,
                node_port,
                node_registry,
                peers,
                rewards_address,
                rpc_address,
                rpc_port,
                skip_reachability_check,
                path,
                no_upnp,
                url,
                user,
                version,
                verbosity,
                write_older_cache_files,
            )
            .await?;
            Ok(())
        }
        Some(SubCmd::Balance {
            peer_id: peer_ids,
            service_name: service_names,
        }) => cmd::node::balance(peer_ids, node_registry, service_names, verbosity).await,
        Some(SubCmd::Local(local_command)) => match local_command {
            LocalSubCmd::Join {
                build,
                count,
                interval,
                metrics_port,
                node_path,
                node_port,
                node_version,
                log_format,
                peers,
                rpc_port,
                rewards_address,
                evm_network,
                skip_validation: _,
            } => {
                let evm_network = evm_network
                    .unwrap_or(EvmNetworkCommand::EvmLocal)
                    .try_into()?;

                cmd::local::join(
                    build,
                    count,
                    interval,
                    metrics_port,
                    node_path,
                    node_port,
                    node_version,
                    log_format,
                    peers,
                    rpc_port,
                    rewards_address,
                    evm_network,
                    true,
                    verbosity,
                )
                .await
            }
            LocalSubCmd::Kill { keep_directories } => {
                cmd::local::kill(keep_directories, verbosity).await
            }
            LocalSubCmd::Run {
                build,
                clean,
                count,
                interval,
                log_format,
                metrics_port,
                node_path,
                node_port,
                node_version,
                rpc_port,
                rewards_address,
                evm_network,
                skip_validation: _,
            } => {
                let evm_network = evm_network
                    .unwrap_or(EvmNetworkCommand::EvmLocal)
                    .try_into()?;

                cmd::local::run(
                    build,
                    clean,
                    count,
                    interval,
                    metrics_port,
                    node_path,
                    node_port,
                    node_version,
                    log_format,
                    rpc_port,
                    rewards_address,
                    evm_network,
                    true,
                    verbosity,
                )
                .await
            }
            LocalSubCmd::Status {
                details,
                fail,
                json,
            } => cmd::local::status(details, fail, json).await,
        },
        Some(SubCmd::Remove {
            keep_directories,
            peer_id: peer_ids,
            service_name: service_names,
        }) => {
            cmd::node::remove(
                keep_directories,
                peer_ids,
                node_registry,
                service_names,
                verbosity,
            )
            .await
        }
        Some(SubCmd::Reset { force }) => cmd::node::reset(force, node_registry, verbosity).await,
        Some(SubCmd::Start {
            interval,
            peer_id: peer_ids,
            service_name: service_names,
        }) => cmd::node::start(interval, node_registry, peer_ids, service_names, verbosity).await,
        Some(SubCmd::Status {
            details,
            fail,
            json,
        }) => cmd::node::status(details, fail, json, node_registry).await,
        Some(SubCmd::Stop {
            interval,
            peer_id: peer_ids,
            service_name: service_names,
        }) => cmd::node::stop(interval, node_registry, peer_ids, service_names, verbosity).await,
        Some(SubCmd::Upgrade {
            do_not_start,
            force,
            interval,
            path,
            peer_id: peer_ids,
            service_name: service_names,
            env_variables: provided_env_variable,
            url,
            version,
        }) => {
            cmd::node::upgrade(
                do_not_start,
                path,
                force,
                interval,
                node_registry,
                peer_ids,
                provided_env_variable,
                service_names,
                url,
                version,
                verbosity,
            )
            .await
        }
        None => Ok(()),
    }
}

fn get_log_builder(level: Level) -> Result<LogBuilder> {
    let logging_targets = vec![
        ("ant_bootstrap".to_string(), level),
        ("evmlib".to_string(), level),
        ("evm-testnet".to_string(), level),
        ("ant_node_manager".to_string(), level),
        ("antctl".to_string(), level),
        ("ant_service_management".to_string(), level),
    ];
    let mut log_builder = LogBuilder::new(logging_targets);
    log_builder.output_dest(ant_logging::LogOutputDest::Stderr);
    log_builder.print_updates_to_stdout(false);
    Ok(log_builder)
}

// Since delimiter is on, we get element of the csv and not the entire csv.
fn parse_environment_variables(env_var: &str) -> Result<(String, String)> {
    let parts: Vec<&str> = env_var.splitn(2, '=').collect();
    if parts.len() != 2 {
        return Err(eyre!(
            "Environment variable must be in the format KEY=VALUE or KEY=INNER_KEY=VALUE.\nMultiple key-value pairs can be given with a comma between them."
        ));
    }
    Ok((parts[0].to_string(), parts[1].to_string()))
}

#[cfg(windows)]
async fn configure_winsw(verbosity: VerbosityLevel) -> Result<()> {
    use ant_node_manager::config::get_node_manager_path;

    // If the node manager was installed using `antup`, it would have put the winsw.exe binary at
    // `C:\Users\<username>\autonomi\winsw.exe`, sitting it alongside the other safe-related binaries.
    //
    // However, if the node manager has been obtained by other means, we can put winsw.exe
    // alongside the directory where the services are defined. This prevents creation of what would
    // seem like a random `autonomi` directory in the user's home directory.
    let antup_winsw_path = dirs_next::home_dir()
        .ok_or_else(|| eyre!("Could not obtain user home directory"))?
        .join("autonomi")
        .join("winsw.exe");
    if antup_winsw_path.exists() {
        ant_node_manager::helpers::configure_winsw(&antup_winsw_path, verbosity).await?;
    } else {
        ant_node_manager::helpers::configure_winsw(
            &get_node_manager_path()?.join("winsw.exe"),
            verbosity,
        )
        .await?;
    }
    Ok(())
}

#[cfg(not(windows))]
#[allow(clippy::unused_async)]
async fn configure_winsw(_verbosity: VerbosityLevel) -> Result<()> {
    Ok(())
}
