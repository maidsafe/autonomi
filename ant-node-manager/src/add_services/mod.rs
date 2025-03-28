// Copyright (C) 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.
pub mod config;
#[cfg(test)]
mod tests;

use self::config::{
    AddAuditorServiceOptions, AddDaemonServiceOptions, AddFaucetServiceOptions,
    AddNodeServiceOptions, InstallAuditorServiceCtxBuilder, InstallFaucetServiceCtxBuilder,
    InstallNodeServiceCtxBuilder,
};
use crate::{
    config::{create_owned_dir, get_user_antnode_data_dir},
    helpers::{check_port_availability, get_start_port_if_applicable, increment_port_option},
    VerbosityLevel, DAEMON_SERVICE_NAME,
};
use ant_service_management::{
    auditor::AuditorServiceData, control::ServiceControl, DaemonServiceData, FaucetServiceData,
    NatDetectionStatus, NodeRegistry, NodeServiceData, ServiceStatus,
};
use color_eyre::{
    eyre::{eyre, OptionExt},
    Help, Result,
};
use colored::Colorize;
use service_manager::ServiceInstallCtx;
use std::{
    ffi::OsString,
    net::{IpAddr, SocketAddr},
};

/// Install antnode as a service.
///
/// This only defines the service; it does not start it.
///
/// There are several arguments that probably seem like they could be handled within the function,
/// but they enable more controlled unit testing.
///
/// Returns the service names of the added services.
pub async fn add_node(
    mut options: AddNodeServiceOptions,
    node_registry: &mut NodeRegistry,
    service_control: &dyn ServiceControl,
    verbosity: VerbosityLevel,
) -> Result<Vec<String>> {
    if options.init_peers_config.first {
        if let Some(count) = options.count {
            if count > 1 {
                error!("A genesis node can only be added as a single node");
                return Err(eyre!("A genesis node can only be added as a single node"));
            }
        }

        let genesis_node = node_registry.nodes.iter().find(|n| n.peers_args.first);
        if genesis_node.is_some() {
            error!("A genesis node already exists");
            return Err(eyre!("A genesis node already exists"));
        }
    }

    if let Some(port_option) = &options.node_port {
        port_option.validate(options.count.unwrap_or(1))?;
        check_port_availability(port_option, &node_registry.nodes)?;
    }

    if let Some(port_option) = &options.metrics_port {
        port_option.validate(options.count.unwrap_or(1))?;
        check_port_availability(port_option, &node_registry.nodes)?;
    }

    let antnode_file_name = options
        .antnode_src_path
        .file_name()
        .ok_or_else(|| {
            error!("Could not get filename from the antnode download path");
            eyre!("Could not get filename from the antnode download path")
        })?
        .to_string_lossy()
        .to_string();

    if options.env_variables.is_some() {
        node_registry
            .environment_variables
            .clone_from(&options.env_variables);
        node_registry.save()?;
    }

    let mut added_service_data = vec![];
    let mut failed_service_data = vec![];

    let current_node_count = node_registry.nodes.len() as u16;
    let target_node_count = current_node_count + options.count.unwrap_or(1);

    let mut node_number = current_node_count + 1;
    let mut node_port = get_start_port_if_applicable(options.node_port);
    let mut metrics_port = get_start_port_if_applicable(options.metrics_port);

    while node_number <= target_node_count {
        trace!("Adding node with node_number {node_number}");
        let metrics_free_port = if let Some(port) = metrics_port {
            Some(port)
        } else {
            Some(service_control.get_available_port()?)
        };

        let service_name = format!("antnode{node_number}");
        let service_data_dir_path = options.service_data_dir_path.join(service_name.clone());
        let service_antnode_path = service_data_dir_path.join(antnode_file_name.clone());

        // For a user mode service, if the user has *not* specified a custom directory and they are
        // using the default, e.g., ~/.local/share/autonomi/node/<service-name>, an additional "logs"
        // directory needs to be appended to the path, otherwise the log files will be output at
        // the same directory where `secret-key` is, which is not what users expect.
        let default_log_dir_path = get_user_antnode_data_dir()?;
        let service_log_dir_path =
            if options.user_mode && options.service_log_dir_path == default_log_dir_path {
                options
                    .service_log_dir_path
                    .join(service_name.clone())
                    .join("logs")
            } else {
                options.service_log_dir_path.join(service_name.clone())
            };

        if let Some(user) = &options.user {
            debug!("Creating data_dir and log_dirs with user {user}");
            create_owned_dir(service_data_dir_path.clone(), user)?;
            create_owned_dir(service_log_dir_path.clone(), user)?;
        } else {
            debug!("Creating data_dir and log_dirs without user");
            std::fs::create_dir_all(service_data_dir_path.clone())?;
            std::fs::create_dir_all(service_log_dir_path.clone())?;
        }

        debug!("Copying antnode binary to {service_antnode_path:?}");
        std::fs::copy(
            options.antnode_src_path.clone(),
            service_antnode_path.clone(),
        )?;

        if options.auto_set_nat_flags {
            let nat_status = node_registry
                .nat_status
                .clone()
                .ok_or_eyre("NAT status has not been set. Run 'nat-detection' first")?;

            match nat_status {
                NatDetectionStatus::Public => {
                    options.no_upnp = true; // UPnP not needed
                    options.relay = false;
                }
                NatDetectionStatus::UPnP => {
                    options.no_upnp = false;
                    options.relay = false;
                }
                NatDetectionStatus::Private => {
                    options.no_upnp = true;
                    options.relay = true;
                }
            }
            debug!(
                "Auto-setting NAT flags: upnp={}, relay={}",
                !options.no_upnp, options.relay
            );
        }

        let install_ctx = InstallNodeServiceCtxBuilder {
            autostart: options.auto_restart,
            data_dir_path: service_data_dir_path.clone(),
            env_variables: options.env_variables.clone(),
            evm_network: options.evm_network.clone(),
            home_network: options.relay,
            log_dir_path: service_log_dir_path.clone(),
            log_format: options.log_format,
            max_archived_log_files: options.max_archived_log_files,
            max_log_files: options.max_log_files,
            metrics_port: metrics_free_port,
            name: service_name.clone(),
            network_id: options.network_id,
            node_ip: options.node_ip,
            node_port,
            init_peers_config: options.init_peers_config.clone(),
            rewards_address: options.rewards_address,
            antnode_path: service_antnode_path.clone(),
            service_user: options.user.clone(),
            upnp: !options.no_upnp,
        }
        .build()?;

        match service_control.install(install_ctx, options.user_mode) {
            Ok(()) => {
                info!("Successfully added service {service_name}");
                added_service_data.push((
                    service_name.clone(),
                    service_antnode_path.to_string_lossy().into_owned(),
                    service_data_dir_path.to_string_lossy().into_owned(),
                    service_log_dir_path.to_string_lossy().into_owned(),
                ));

                node_registry.nodes.push(NodeServiceData {
                    antnode_path: service_antnode_path,
                    auto_restart: options.auto_restart,
                    connected_peers: None,
                    data_dir_path: service_data_dir_path.clone(),
                    evm_network: options.evm_network.clone(),
                    home_network: options.relay,
                    listen_addr: None,
                    log_dir_path: service_log_dir_path.clone(),
                    log_format: options.log_format,
                    max_archived_log_files: options.max_archived_log_files,
                    max_log_files: options.max_log_files,
                    metrics_port: metrics_free_port,
                    network_id: options.network_id,
                    node_ip: options.node_ip,
                    node_port,
                    number: node_number,
                    rewards_address: options.rewards_address,
                    reward_balance: None,
                    rpc_socket_addr: None,
                    peer_id: None,
                    peers_args: options.init_peers_config.clone(),
                    pid: None,
                    service_name,
                    status: ServiceStatus::Added,
                    upnp: !options.no_upnp,
                    user: options.user.clone(),
                    user_mode: options.user_mode,
                    version: options.version.clone(),
                });
                // We save the node registry for each service because it's possible any number of
                // services could fail to be added.
                node_registry.save()?;
            }
            Err(e) => {
                error!("Failed to add service {service_name}: {e}");
                failed_service_data.push((service_name.clone(), e.to_string()));
            }
        }

        node_number += 1;
        node_port = increment_port_option(node_port);
        metrics_port = increment_port_option(metrics_port);
    }

    if options.delete_antnode_src {
        debug!("Deleting antnode binary file");
        std::fs::remove_file(options.antnode_src_path)?;
    }

    if !added_service_data.is_empty() {
        info!("Added {} services", added_service_data.len());
    } else if !failed_service_data.is_empty() {
        error!("Failed to add {} service(s)", failed_service_data.len());
    }

    if !added_service_data.is_empty() && verbosity != VerbosityLevel::Minimal {
        println!("Services Added:");
        for install in added_service_data.iter() {
            println!(" {} {}", "✓".green(), install.0);
            println!("    - Antnode path: {}", install.1);
            println!("    - Data path: {}", install.2);
            println!("    - Log path: {}", install.3);
        }
        println!("[!] Note: newly added services have not been started");
    }

    if !failed_service_data.is_empty() {
        if verbosity != VerbosityLevel::Minimal {
            println!("Failed to add {} service(s):", failed_service_data.len());
            for failed in failed_service_data.iter() {
                println!("{} {}: {}", "✕".red(), failed.0, failed.1);
            }
        }
        return Err(eyre!("Failed to add one or more services")
            .suggestion("However, any services that were successfully added will be usable."));
    }

    let added_services_names = added_service_data
        .into_iter()
        .map(|(name, ..)| name)
        .collect();

    Ok(added_services_names)
}

/// Install the auditor as a service.
///
/// This only defines the service; it does not start it.
///
/// There are several arguments that probably seem like they could be handled within the function,
/// but they enable more controlled unit testing.
pub fn add_auditor(
    install_options: AddAuditorServiceOptions,
    node_registry: &mut NodeRegistry,
    service_control: &dyn ServiceControl,
    verbosity: VerbosityLevel,
) -> Result<()> {
    if node_registry.auditor.is_some() {
        error!("An Auditor service has already been created");
        return Err(eyre!("An Auditor service has already been created"));
    }

    debug!(
        "Creating log directory at {:?} as user {:?}",
        install_options.service_log_dir_path, install_options.user
    );
    create_owned_dir(
        install_options.service_log_dir_path.clone(),
        &install_options.user,
    )?;

    debug!(
        "Copying auditor binary file to {:?}",
        install_options.auditor_install_bin_path
    );
    std::fs::copy(
        install_options.auditor_src_bin_path.clone(),
        install_options.auditor_install_bin_path.clone(),
    )?;

    let install_ctx = InstallAuditorServiceCtxBuilder {
        auditor_path: install_options.auditor_install_bin_path.clone(),
        beta_encryption_key: install_options.beta_encryption_key.clone(),
        env_variables: install_options.env_variables.clone(),
        log_dir_path: install_options.service_log_dir_path.clone(),
        name: "auditor".to_string(),
        service_user: install_options.user.clone(),
    }
    .build()?;

    match service_control.install(install_ctx, false) {
        Ok(()) => {
            node_registry.auditor = Some(AuditorServiceData {
                auditor_path: install_options.auditor_install_bin_path.clone(),
                log_dir_path: install_options.service_log_dir_path.clone(),
                pid: None,
                service_name: "auditor".to_string(),
                status: ServiceStatus::Added,
                user: install_options.user.clone(),
                version: install_options.version,
            });
            info!("Auditor service has been added successfully");
            println!("Auditor service added {}", "✓".green());
            if verbosity != VerbosityLevel::Minimal {
                println!(
                    "  - Bin path: {}",
                    install_options.auditor_install_bin_path.to_string_lossy()
                );
                println!(
                    "  - Log path: {}",
                    install_options.service_log_dir_path.to_string_lossy()
                );
            }
            println!("[!] Note: the service has not been started");
            debug!("Removing auditor binary file");
            std::fs::remove_file(install_options.auditor_src_bin_path)?;
            node_registry.save()?;
            Ok(())
        }
        Err(e) => {
            error!("Failed to add auditor service: {e}");
            println!("Failed to add auditor service: {e}");
            Err(e.into())
        }
    }
}

/// Install the daemon as a service.
///
/// This only defines the service; it does not start it.
pub fn add_daemon(
    options: AddDaemonServiceOptions,
    node_registry: &mut NodeRegistry,
    service_control: &dyn ServiceControl,
) -> Result<()> {
    if node_registry.daemon.is_some() {
        error!("A antctld service has already been created");
        return Err(eyre!("A antctld service has already been created"));
    }

    debug!(
        "Copying daemon binary file to {:?}",
        options.daemon_install_bin_path
    );
    std::fs::copy(
        options.daemon_src_bin_path.clone(),
        options.daemon_install_bin_path.clone(),
    )?;

    let install_ctx = ServiceInstallCtx {
        args: vec![
            OsString::from("--port"),
            OsString::from(options.port.to_string()),
            OsString::from("--address"),
            OsString::from(options.address.to_string()),
        ],
        autostart: true,
        contents: None,
        environment: options.env_variables,
        label: DAEMON_SERVICE_NAME.parse()?,
        program: options.daemon_install_bin_path.clone(),
        username: Some(options.user),
        working_directory: None,
        disable_restart_on_failure: false,
    };

    match service_control.install(install_ctx, false) {
        Ok(()) => {
            let daemon = DaemonServiceData {
                daemon_path: options.daemon_install_bin_path.clone(),
                endpoint: Some(SocketAddr::new(IpAddr::V4(options.address), options.port)),
                pid: None,
                service_name: DAEMON_SERVICE_NAME.to_string(),
                status: ServiceStatus::Added,
                version: options.version,
            };
            node_registry.daemon = Some(daemon);
            info!("Daemon service has been added successfully");
            println!("Daemon service added {}", "✓".green());
            println!("[!] Note: the service has not been started");
            node_registry.save()?;
            std::fs::remove_file(options.daemon_src_bin_path)?;
            Ok(())
        }
        Err(e) => {
            error!("Failed to add daemon service: {e}");
            println!("Failed to add daemon service: {e}");
            Err(e.into())
        }
    }
}

/// Install the faucet as a service.
///
/// This only defines the service; it does not start it.
///
/// There are several arguments that probably seem like they could be handled within the function,
/// but they enable more controlled unit testing.
pub fn add_faucet(
    install_options: AddFaucetServiceOptions,
    node_registry: &mut NodeRegistry,
    service_control: &dyn ServiceControl,
    verbosity: VerbosityLevel,
) -> Result<()> {
    if node_registry.faucet.is_some() {
        error!("A faucet service has already been created");
        return Err(eyre!("A faucet service has already been created"));
    }

    debug!(
        "Creating log directory at {:?} as user {:?}",
        install_options.service_log_dir_path, install_options.user
    );
    create_owned_dir(
        install_options.service_log_dir_path.clone(),
        &install_options.user,
    )?;
    debug!(
        "Copying faucet binary file to {:?}",
        install_options.faucet_install_bin_path
    );
    std::fs::copy(
        install_options.faucet_src_bin_path.clone(),
        install_options.faucet_install_bin_path.clone(),
    )?;

    let install_ctx = InstallFaucetServiceCtxBuilder {
        env_variables: install_options.env_variables.clone(),
        faucet_path: install_options.faucet_install_bin_path.clone(),
        local: install_options.local,
        log_dir_path: install_options.service_log_dir_path.clone(),
        name: "faucet".to_string(),
        service_user: install_options.user.clone(),
    }
    .build()?;

    match service_control.install(install_ctx, false) {
        Ok(()) => {
            node_registry.faucet = Some(FaucetServiceData {
                faucet_path: install_options.faucet_install_bin_path.clone(),
                local: false,
                log_dir_path: install_options.service_log_dir_path.clone(),
                pid: None,
                service_name: "faucet".to_string(),
                status: ServiceStatus::Added,
                user: install_options.user.clone(),
                version: install_options.version,
            });
            info!("Faucet service has been added successfully");
            println!("Faucet service added {}", "✓".green());
            if verbosity != VerbosityLevel::Minimal {
                println!(
                    "  - Bin path: {}",
                    install_options.faucet_install_bin_path.to_string_lossy()
                );
                println!(
                    "  - Data path: {}",
                    install_options.service_data_dir_path.to_string_lossy()
                );
                println!(
                    "  - Log path: {}",
                    install_options.service_log_dir_path.to_string_lossy()
                );
            }
            println!("[!] Note: the service has not been started");
            std::fs::remove_file(install_options.faucet_src_bin_path)?;
            node_registry.save()?;
            Ok(())
        }
        Err(e) => {
            error!("Failed to add faucet service: {e}");
            println!("Failed to add faucet service: {e}");
            Err(e.into())
        }
    }
}
