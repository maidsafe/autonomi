// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use autonomi::client::config::ClientOperatingStrategy;
use autonomi::{get_evm_network, Client, ClientConfig, InitialPeersConfig};
use color_eyre::eyre::eyre;
use indicatif::ProgressBar;
use std::time::Duration;

use crate::exit_code::{connect_error_exit_code, evm_util_error_exit_code, ExitCodeError};

pub async fn connect_to_network(
    init_peers_config: InitialPeersConfig,
    network_id: Option<u8>,
) -> Result<Client, ExitCodeError> {
    connect_to_network_with_config(init_peers_config, Default::default(), network_id).await
}

pub async fn connect_to_network_with_config(
    init_peers_config: InitialPeersConfig,
    operation_config: ClientOperatingStrategy,
    network_id: Option<u8>,
) -> Result<Client, ExitCodeError> {
    let progress_bar = ProgressBar::new_spinner();
    progress_bar.enable_steady_tick(Duration::from_millis(120));
    progress_bar.set_message("Connecting to The Autonomi Network...");
    let new_style = progress_bar.style().tick_chars("⠁⠂⠄⡀⢀⠠⠐⠈🔗");
    progress_bar.set_style(new_style);

    if init_peers_config.local {
        progress_bar.set_message("Connecting to a local Autonomi Network...");
    } else {
        progress_bar.set_message("Connecting to The Autonomi Network...");
    };

    let evm_network = get_evm_network(init_peers_config.local).map_err(|err| {
        let exit_code = evm_util_error_exit_code(&err);
        (err.into(), exit_code)
    })?;

    let config = ClientConfig {
        init_peers_config,
        evm_network,
        strategy: operation_config,
        network_id,
    };

    let res = Client::init_with_config(config).await;

    match res {
        Ok(client) => {
            info!("Connected to the Network");
            progress_bar.finish_with_message("Connected to the Network");
            Ok(client)
        }
        Err(e) => {
            error!("Failed to connect to the network: {e}");
            progress_bar.finish_with_message("Failed to connect to the network");
            let exit_code = connect_error_exit_code(&e);
            Err((
                eyre!(e).wrap_err("Failed to connect to the network"),
                exit_code,
            ))
        }
    }
}
