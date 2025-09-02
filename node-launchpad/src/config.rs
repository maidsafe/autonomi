// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::connection_mode::ConnectionMode;
use crate::system::get_primary_mount_point;
use ant_evm::EvmAddress;
use ant_node_manager::config::is_running_as_root;
use color_eyre::eyre::{Result, eyre};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Where to store the Nodes data.
///
/// If `base_dir` is the primary mount point, we store in "<base_dir>/$HOME/user_data_dir/autonomi/node".
///
/// if not we store in "<base_dir>/autonomi/node".
///
/// If should_create is true, the directory will be created if it doesn't exists.
pub fn get_launchpad_nodes_data_dir_path(
    base_dir: &PathBuf,
    should_create: bool,
) -> Result<PathBuf> {
    let mut mount_point = PathBuf::new();
    let is_root = is_running_as_root();

    let data_directory: PathBuf = if *base_dir == get_primary_mount_point() {
        if is_root {
            // The root's data directory isn't accessible to the user `ant`, so we are using an
            // alternative default path that `ant` can access.
            #[cfg(unix)]
            {
                let default_data_dir_path = PathBuf::from("/var/antctl/services");
                debug!(
                    "Running as root; using default path {:?} for nodes data directory instead of primary mount point",
                    default_data_dir_path
                );
                default_data_dir_path
            }
            #[cfg(windows)]
            get_user_data_dir()?
        } else {
            get_user_data_dir()?
        }
    } else {
        base_dir.clone()
    };
    mount_point.push(data_directory);
    mount_point.push("autonomi");
    mount_point.push("node");
    if should_create {
        debug!("Creating nodes data dir: {:?}", mount_point.as_path());
        match std::fs::create_dir_all(mount_point.as_path()) {
            Ok(_) => debug!("Nodes {:?} data dir created successfully", mount_point),
            Err(e) => {
                error!(
                    "Failed to create nodes data dir in {:?}: {:?}",
                    mount_point, e
                );
                return Err(eyre!(
                    "Failed to create nodes data dir in {:?}",
                    mount_point
                ));
            }
        }
    }
    Ok(mount_point)
}

fn get_user_data_dir() -> Result<PathBuf> {
    dirs_next::data_dir().ok_or_else(|| eyre!("User data directory is not obtainable",))
}

/// Where to store the Launchpad config & logs.
///
pub fn get_launchpad_data_dir_path() -> Result<PathBuf> {
    let mut home_dirs =
        dirs_next::data_dir().ok_or_else(|| eyre!("Data directory is not obtainable"))?;
    home_dirs.push("autonomi");
    home_dirs.push("launchpad");
    std::fs::create_dir_all(home_dirs.as_path())?;
    Ok(home_dirs)
}

pub fn get_config_dir() -> Result<PathBuf> {
    // TODO: consider using dirs_next::config_dir. Configuration and data are different things.
    let config_dir = get_launchpad_data_dir_path()?.join("config");
    std::fs::create_dir_all(&config_dir)?;
    Ok(config_dir)
}

#[cfg(windows)]
pub async fn configure_winsw() -> Result<()> {
    let data_dir_path = get_launchpad_data_dir_path()?;
    ant_node_manager::helpers::configure_winsw(
        &data_dir_path.join("winsw.exe"),
        ant_node_manager::VerbosityLevel::Minimal,
    )
    .await?;
    Ok(())
}

#[cfg(not(windows))]
pub async fn configure_winsw() -> Result<()> {
    Ok(())
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AppData {
    pub rewards_address: Option<EvmAddress>,
    pub nodes_to_start: u64,
    pub storage_mountpoint: Option<PathBuf>,
    pub storage_drive: Option<String>,
    pub connection_mode: Option<ConnectionMode>,
    pub port_from: Option<u32>,
    pub port_to: Option<u32>,
}

impl Default for AppData {
    fn default() -> Self {
        Self {
            rewards_address: None,
            nodes_to_start: 1,
            storage_mountpoint: None,
            storage_drive: None,
            connection_mode: None,
            port_from: None,
            port_to: None,
        }
    }
}

impl AppData {
    pub fn load(custom_path: Option<PathBuf>) -> Result<Self> {
        let config_path = if let Some(path) = custom_path {
            path
        } else {
            get_config_dir()
                .map_err(|_| color_eyre::eyre::eyre!("Could not obtain config dir"))?
                .join("app_data.json")
        };

        if !config_path.exists() {
            return Ok(Self::default());
        }

        let data = std::fs::read_to_string(&config_path).map_err(|e| {
            error!("Failed to read app data file: {}", e);
            color_eyre::eyre::eyre!("Failed to read app data file: {}", e)
        })?;

        let app_data: AppData = serde_json::from_str(&data).map_err(|e| {
            error!("Failed to parse app data: {}", e);
            color_eyre::eyre::eyre!("Failed to parse app data: {}", e)
        })?;

        Ok(app_data)
    }

    pub fn save(&self, custom_path: Option<PathBuf>) -> Result<()> {
        let config_path = if let Some(path) = custom_path {
            path
        } else {
            get_config_dir()
                .map_err(|_| config::ConfigError::Message("Could not obtain data dir".to_string()))?
                .join("app_data.json")
        };

        let serialized_config = serde_json::to_string_pretty(&self)?;
        std::fs::write(config_path, serialized_config)?;

        Ok(())
    }
}
