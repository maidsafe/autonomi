// Copyright (C) 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::{
    BatchServiceManager, VerbosityLevel,
    add_services::{add_daemon, config::AddDaemonServiceOptions},
    config::{self, is_running_as_root},
    helpers::{download_and_extract_release, get_bin_version},
    print_banner,
};
use ant_releases::{AntReleaseRepoActions, ReleaseType};
use ant_service_management::{
    DaemonService, NodeRegistryManager,
    control::{ServiceControl, ServiceController},
};
use color_eyre::{Result, eyre::eyre};
use std::{net::Ipv4Addr, path::PathBuf, sync::Arc};

pub async fn add(
    address: Ipv4Addr,
    env_variables: Option<Vec<(String, String)>>,
    port: u16,
    src_path: Option<PathBuf>,
    url: Option<String>,
    version: Option<String>,
    verbosity: VerbosityLevel,
) -> Result<()> {
    if !is_running_as_root() {
        error!("The daemon add command must run as the root user");
        return Err(eyre!("The add command must run as the root user"));
    }

    if verbosity != VerbosityLevel::Minimal {
        print_banner("Add Daemon Service");
    }

    let service_user = "ant";
    let service_manager = ServiceController {};
    debug!("Trying to create service user '{service_user}' for the daemon");
    service_manager.create_service_user(service_user)?;

    let node_registry = NodeRegistryManager::load(&config::get_node_registry_path()?).await?;
    let release_repo = <dyn AntReleaseRepoActions>::default_config();

    let (daemon_src_bin_path, version) = if let Some(path) = src_path {
        let version = get_bin_version(&path)?;
        (path, version)
    } else {
        download_and_extract_release(
            ReleaseType::AntCtlDaemon,
            url.clone(),
            version,
            &*release_repo,
            verbosity,
            None,
        )
        .await?
    };

    info!("Adding daemon service");

    // At the moment we don't have the option to provide a user for running the service. Since
    // `antctld` requires manipulation of services, the user running it must either be root or
    // have root access. For now we will just use the `root` user. The user option gets ignored on
    // Windows anyway, so there shouldn't be a cross-platform issue here.
    add_daemon(
        AddDaemonServiceOptions {
            address,
            env_variables,
            daemon_install_bin_path: config::get_daemon_install_path(),
            daemon_src_bin_path,
            port,
            user: "root".to_string(),
            version,
        },
        node_registry,
        &ServiceController {},
    )
    .await?;
    Ok(())
}

pub async fn start(verbosity: VerbosityLevel) -> Result<()> {
    if !is_running_as_root() {
        error!("The daemon start command must run as the root user");
        return Err(eyre!("The start command must run as the root user"));
    }

    let node_registry = NodeRegistryManager::load(&config::get_node_registry_path()?).await?;

    if let Some(daemon) = node_registry.daemon.read().await.as_ref() {
        if verbosity != VerbosityLevel::Minimal {
            print_banner("Start Daemon Service");
        }
        info!("Starting daemon service");

        let service = DaemonService::new(Arc::clone(daemon), Box::new(ServiceController {}));
        let service_manager = BatchServiceManager::new(
            vec![service],
            Box::new(ServiceController {}),
            node_registry.clone(),
            verbosity,
        );
        let result = service_manager.start_all(0).await;
        result.into_eyre()?;

        println!(
            "Endpoint: {}",
            daemon
                .read()
                .await
                .endpoint
                .map_or("-".to_string(), |e| e.to_string())
        );

        node_registry.save().await?;
        return Ok(());
    }

    error!("The daemon service has not been added yet");
    Err(eyre!("The daemon service has not been added yet"))
}

pub async fn stop(verbosity: VerbosityLevel) -> Result<()> {
    if !is_running_as_root() {
        error!("The daemon stop command must run as the root user");
        return Err(eyre!("The stop command must run as the root user"));
    }

    let node_registry = NodeRegistryManager::load(&config::get_node_registry_path()?).await?;
    if let Some(daemon) = node_registry.daemon.read().await.as_ref() {
        if verbosity != VerbosityLevel::Minimal {
            print_banner("Stop Daemon Service");
        }
        info!("Stopping daemon service");

        let service = DaemonService::new(Arc::clone(daemon), Box::new(ServiceController {}));
        let service_manager = BatchServiceManager::new(
            vec![service],
            Box::new(ServiceController {}),
            node_registry.clone(),
            verbosity,
        );
        service_manager.stop_all(None).await;

        node_registry.save().await?;

        return Ok(());
    }

    error!("The daemon service has not been added yet");
    Err(eyre!("The daemon service has not been added yet"))
}
