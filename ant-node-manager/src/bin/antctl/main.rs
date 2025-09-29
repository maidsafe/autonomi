// Copyright (C) 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

mod commands;

use crate::commands::*;
use ant_logging::{LogBuilder, LogOutputDest};
use ant_node_manager::{
    VerbosityLevel, cmd,
    config::{self, get_node_manager_path},
};
use ant_service_management::NodeRegistryManager;
use clap::Parser;
use color_eyre::eyre::{Result, eyre};
use tracing::Level;

#[derive(Debug, Clone)]
pub enum LogOutputDestArg {
    StdOut,
    StdErr,
    DataDir,
}

impl std::fmt::Display for LogOutputDestArg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LogOutputDestArg::StdOut => write!(f, "stdout"),
            LogOutputDestArg::StdErr => write!(f, "stderr"),
            LogOutputDestArg::DataDir => write!(f, "data-dir"),
        }
    }
}

pub fn parse_log_output(val: &str) -> Result<LogOutputDestArg> {
    match val {
        "stdout" => Ok(LogOutputDestArg::StdOut),
        "stderr" => Ok(LogOutputDestArg::StdErr),
        "data-dir" => Ok(LogOutputDestArg::DataDir),
        _ => Err(eyre!("Invalid log output destination: {val}")),
    }
}

#[tracing::instrument(err)]
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
        let log_output_dest = match &args.log_output_dest {
            LogOutputDestArg::StdOut => LogOutputDest::Stdout,
            LogOutputDestArg::StdErr => LogOutputDest::Stderr,
            LogOutputDestArg::DataDir => {
                let timestamp = chrono::Local::now().format("%Y-%m-%d_%H-%M-%S").to_string();
                let dir = get_node_manager_path()?.join("logs").join(format!(
                    "antctl_{}_{timestamp}.log",
                    args.cmd
                        .as_ref()
                        .map(|c| c.to_string())
                        .unwrap_or_else(|| "no_command".to_string())
                ));
                LogOutputDest::Path(dir)
            }
        };

        let level = if args.debug {
            Level::DEBUG
        } else {
            Level::TRACE
        };
        get_log_builder(level, log_output_dest).initialize()?.1
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
        }) => {
            cmd::node::balance(peer_ids, node_registry, service_names, verbosity).await?;
            Ok(())
        }
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
            service_name,
        }) => {
            cmd::node::remove(
                keep_directories,
                peer_ids,
                node_registry,
                service_name,
                verbosity,
            )
            .await?;

            Ok(())
        }
        Some(SubCmd::Reset { force }) => {
            cmd::node::reset(force, node_registry, verbosity).await?;
            Ok(())
        }
        Some(SubCmd::Start {
            interval,
            peer_id: peer_ids,
            service_name,
            startup_check,
        }) => {
            cmd::node::start(
                interval,
                node_registry,
                peer_ids,
                service_name,
                startup_check,
                verbosity,
            )
            .await?;
            Ok(())
        }
        Some(SubCmd::Status {
            details,
            fail,
            json,
        }) => {
            cmd::node::status(details, fail, json, node_registry).await?;
            Ok(())
        }
        Some(SubCmd::Stop {
            interval,
            peer_id: peer_ids,
            service_name,
        }) => {
            cmd::node::stop(interval, node_registry, peer_ids, service_name, verbosity).await?;
            Ok(())
        }
        Some(SubCmd::Upgrade {
            do_not_start,
            force,
            interval,
            path,
            peer_id: peer_ids,
            service_name,
            env_variables: provided_env_variable,
            url,
            startup_check,
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
                service_name,
                startup_check,
                url,
                version,
                verbosity,
            )
            .await?;
            Ok(())
        }
        None => Err(eyre!(
            "No command provided, try --help for more information"
        )),
    }
}

fn get_log_builder(level: Level, log_output_dest: LogOutputDest) -> LogBuilder {
    let logging_targets = vec![
        ("ant_bootstrap".to_string(), level),
        ("evmlib".to_string(), level),
        ("evm-testnet".to_string(), level),
        ("ant_node_manager".to_string(), level),
        ("antctl".to_string(), level),
        ("ant_service_management".to_string(), level),
        ("service-manager".to_string(), level),
    ];
    let mut log_builder = LogBuilder::new(logging_targets);
    log_builder.output_dest(log_output_dest);
    log_builder.print_updates_to_stdout(false);
    log_builder
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
#[tracing::instrument(fields(verbosity = ?verbosity), err)]
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
#[tracing::instrument(skip(_verbosity), err)]
async fn configure_winsw(_verbosity: VerbosityLevel) -> Result<()> {
    Ok(())
}
