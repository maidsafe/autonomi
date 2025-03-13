// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use autonomi::client::config::ClientOperatingStrategy;
use autonomi::{get_evm_network, Client, ClientConfig, InitialPeersConfig};
use color_eyre::eyre::bail;
use color_eyre::eyre::Result;
use indicatif::ProgressBar;
use std::time::Duration;

pub async fn connect_to_network(init_peers_config: InitialPeersConfig) -> Result<Client> {
    connect_to_network_with_config(init_peers_config, Default::default()).await
}

pub async fn connect_to_network_with_config(
    init_peers_config: InitialPeersConfig,
    operation_config: ClientOperatingStrategy,
) -> Result<Client> {
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

    let evm_network = get_evm_network(init_peers_config.local)?;

    let config = ClientConfig {
        init_peers_config,
        evm_network,
        strategy: operation_config,
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
            bail!("Failed to connect to the network: {e}")
        }
    }
}
