// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::exit_code::{connect_error_exit_code, evm_util_error_exit_code, ExitCodeError};
use crate::opt::NetworkId;
use autonomi::client::config::ClientOperatingStrategy;
use autonomi::{get_evm_network, Client, ClientConfig, InitialPeersConfig};
use color_eyre::eyre::eyre;
use indicatif::ProgressBar;
use std::time::Duration;

pub async fn connect_to_network(
    init_peers_config: InitialPeersConfig,
    network_id: NetworkId,
) -> Result<Client, ExitCodeError> {
    connect_to_network_with_config(init_peers_config, Default::default(), network_id).await
}

/// Connect to the network with the given configuration.
/// If the NetworkId is different from Custom, the InitialPeersConfig will be ignored.
pub async fn connect_to_network_with_config(
    init_peers_config: InitialPeersConfig,
    operation_config: ClientOperatingStrategy,
    network_id: NetworkId,
) -> Result<Client, ExitCodeError> {
    let progress_bar = ProgressBar::new_spinner();
    progress_bar.enable_steady_tick(Duration::from_millis(120));
    progress_bar.set_message("Connecting to The Autonomi Network...");
    let new_style = progress_bar.style().tick_chars("⠁⠂⠄⡀⢀⠠⠐⠈🔗");
    progress_bar.set_style(new_style);

    let res = match network_id {
        NetworkId::Local => {
            progress_bar.set_message("Connecting to a local Autonomi Network...");
            Client::init_local().await
        }
        NetworkId::Main => {
            progress_bar.set_message("Connecting to The Autonomi Network...");
            Client::init().await
        }
        NetworkId::Alpha => {
            progress_bar.set_message("Connecting to the Alpha Autonomi Network...");
            Client::init_alpha().await
        }
        NetworkId::Custom => {
            progress_bar.set_message("Connecting to a custom Autonomi Network...");
            let evm_network = get_evm_network(init_peers_config.local).map_err(|err| {
                let exit_code = evm_util_error_exit_code(&err);
                (err.into(), exit_code)
            })?;
            let config = ClientConfig {
                init_peers_config,
                evm_network,
                strategy: operation_config.clone(),
                network_id: None,
            };
            Client::init_with_config(config).await
        }
    };

    match res {
        Ok(client) => {
            info!("Connected to the Network");
            progress_bar.finish_with_message(format!("Connected to the {network_id:?} Network"));
            let client = client.with_strategy(operation_config);
            Ok(client)
        }
        Err(e) => {
            error!("Failed to connect to the network: {e}");
            progress_bar
                .finish_with_message(format!("Failed to connect to the {network_id:?} Network"));
            let exit_code = connect_error_exit_code(&e);
            Err((
                eyre!(e).wrap_err("Failed to connect to the network"),
                exit_code,
            ))
        }
    }
}
