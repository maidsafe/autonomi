// Copyright (C) 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::{
    config::{self, is_running_as_root},
    print_banner, ServiceManager, VerbosityLevel,
};
use ant_bootstrap::InitialPeersConfig;
use ant_service_management::{control::ServiceController, FaucetService, NodeRegistry};
use color_eyre::{eyre::eyre, Result};
use std::path::PathBuf;

pub async fn add(
    _env_variables: Option<Vec<(String, String)>>,
    _log_dir_path: Option<PathBuf>,
    _peers_args: InitialPeersConfig,
    _src_path: Option<PathBuf>,
    _url: Option<String>,
    _version: Option<String>,
    _verbosity: VerbosityLevel,
) -> Result<()> {
    // TODO: The whole subcommand for the auditor should be removed when we have some time.
    panic!("The faucet service is no longer supported");
}

pub async fn start(verbosity: VerbosityLevel) -> Result<()> {
    if !is_running_as_root() {
        error!("The faucet start command must run as the root user");
        return Err(eyre!("The start command must run as the root user"));
    }

    let mut node_registry = NodeRegistry::load(&config::get_node_registry_path()?)?;
    if let Some(faucet) = &mut node_registry.faucet {
        if verbosity != VerbosityLevel::Minimal {
            print_banner("Start Faucet Service");
        }
        info!("Starting faucet service");

        let service = FaucetService::new(faucet, Box::new(ServiceController {}));
        let mut service_manager = ServiceManager::new(
            service,
            Box::new(ServiceController {}),
            VerbosityLevel::Normal,
        );
        service_manager.start().await?;

        node_registry.save()?;
        return Ok(());
    }

    error!("The faucet service has not been added yet");
    Err(eyre!("The faucet service has not been added yet"))
}

pub async fn stop(verbosity: VerbosityLevel) -> Result<()> {
    if !is_running_as_root() {
        error!("The faucet stop command must run as the root user");
        return Err(eyre!("The stop command must run as the root user"));
    }

    let mut node_registry = NodeRegistry::load(&config::get_node_registry_path()?)?;
    if let Some(faucet) = &mut node_registry.faucet {
        if verbosity != VerbosityLevel::Minimal {
            print_banner("Stop Faucet Service");
        }
        info!("Stopping faucet service");

        let service = FaucetService::new(faucet, Box::new(ServiceController {}));
        let mut service_manager =
            ServiceManager::new(service, Box::new(ServiceController {}), verbosity);
        service_manager.stop().await?;

        node_registry.save()?;

        return Ok(());
    }

    error!("The faucet service has not been added yet");
    Err(eyre!("The faucet service has not been added yet"))
}

pub async fn upgrade(
    _do_not_start: bool,
    _force: bool,
    _provided_env_variables: Option<Vec<(String, String)>>,
    _url: Option<String>,
    _version: Option<String>,
    _verbosity: VerbosityLevel,
) -> Result<()> {
    // TODO: The whole subcommand for the auditor should be removed when we have some time.
    panic!("The faucet service is no longer supported");
}
