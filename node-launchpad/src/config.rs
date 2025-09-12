// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::system::get_primary_mount_point;
use ant_evm::EvmAddress;
use ant_node_manager::config::is_running_as_root;
use color_eyre::eyre::{Result, eyre};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing::{debug, error, warn};

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
#[allow(clippy::unused_async)]
pub async fn configure_winsw() -> Result<()> {
    Ok(())
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AppData {
    pub rewards_address: Option<EvmAddress>,
    pub nodes_to_start: u64,
    pub storage_mountpoint: Option<PathBuf>,
    pub storage_drive: Option<String>,
    pub upnp_enabled: bool,
    pub port_range: Option<(u32, u32)>,
}

impl Default for AppData {
    fn default() -> Self {
        Self {
            rewards_address: None,
            nodes_to_start: 1,
            storage_mountpoint: None,
            storage_drive: None,
            upnp_enabled: true,
            port_range: None,
        }
    }
}

impl AppData {
    fn try_salvage_fields(json_value: &serde_json::Value) -> Self {
        let mut salvaged = Self::default();

        if let Some(rewards_addr_value) = json_value.get("rewards_address")
            && let Ok(rewards_addr) =
                serde_json::from_value::<Option<EvmAddress>>(rewards_addr_value.clone())
        {
            salvaged.rewards_address = rewards_addr;
            debug!("Salvaged rewards_address: {:?}", rewards_addr);
        }

        salvaged
    }

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

        match serde_json::from_str::<AppData>(&data) {
            Ok(app_data) => Ok(app_data),
            Err(parse_err) => {
                warn!(
                    "Failed to parse app data due to corruption or structure change: {parse_err:?}, trying to salvage fields...",
                );

                // Try to salvage individual fields using generic JSON parsing
                match serde_json::from_str::<serde_json::Value>(&data) {
                    Ok(json_value) => {
                        let salvaged_data = Self::try_salvage_fields(&json_value);

                        if let Err(save_err) = salvaged_data.save(Some(config_path.clone())) {
                            error!(
                                "Failed to save salvaged app data to {config_path:?}: {save_err:?}"
                            );
                            return Err(eyre!(
                                "Failed to parse corrupted app data and could not save salvaged data: {save_err}",
                            ));
                        }

                        debug!("Successfully saved salvaged app data to {config_path:?}");
                        Ok(salvaged_data)
                    }
                    Err(json_err) => {
                        warn!(
                            "Config file is completely corrupted, cannot salvage any fields: {json_err:?}"
                        );
                        let default_data = Self::default();
                        if let Err(save_err) = default_data.save(Some(config_path.clone())) {
                            error!(
                                "Failed to save fresh app data to {config_path:?}: {save_err:?}"
                            );
                            return Err(eyre!(
                                "Failed to parse corrupted app data and could not save fresh data: {save_err}",
                            ));
                        }

                        debug!("Successfully restored fresh app data to {config_path:?}");
                        Ok(default_data)
                    }
                }
            }
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_salvage_rewards_address_on_missing_field() -> Result<()> {
        let temp_dir = tempdir()?;
        let config_path = temp_dir.path().join("missing_field.json");

        // The exact scenario user reported - missing upnp_enabled field but valid rewards_address
        let config_data = r#"{
    "rewards_address": "0x1234567890abcdef1234567890abcdef12345678",
    "nodes_to_start": 5,
    "storage_mountpoint": "/some/path",
    "storage_drive": "C:",
    "port_range": [12000, 13000]
}"#;
        fs::write(&config_path, config_data)?;

        let app_data = AppData::load(Some(config_path))?;

        // Should salvage rewards_address, use defaults for missing upnp_enabled
        assert_eq!(
            app_data.rewards_address,
            Some(
                "0x1234567890abcdef1234567890abcdef12345678"
                    .parse()
                    .unwrap()
            )
        );
        assert!(app_data.upnp_enabled); // Default value for missing field

        Ok(())
    }

    #[test]
    fn test_fallback_on_complete_corruption() -> Result<()> {
        let temp_dir = tempdir()?;
        let config_path = temp_dir.path().join("corrupted.json");

        // File content that's not JSON at all
        fs::write(&config_path, "this is not json at all!@#$%")?;

        let app_data = AppData::load(Some(config_path))?;

        // Should fall back to full defaults since generic parsing fails
        assert_eq!(app_data.rewards_address, None);
        assert_eq!(app_data.nodes_to_start, 1);
        assert!(app_data.upnp_enabled);

        Ok(())
    }

    #[test]
    fn test_fallback_on_invalid_rewards_address() -> Result<()> {
        let temp_dir = tempdir()?;
        let config_path = temp_dir.path().join("invalid_rewards.json");

        // Valid JSON structure but invalid rewards_address format
        let config_data = r#"{
    "rewards_address": "not_a_valid_ethereum_address",
    "nodes_to_start": 7,
    "upnp_enabled": true
}"#;
        fs::write(&config_path, config_data)?;

        let app_data = AppData::load(Some(config_path))?;

        // Should use default for invalid rewards_address since validation fails
        assert_eq!(app_data.rewards_address, None);
        assert_eq!(app_data.nodes_to_start, 1); // Back to defaults since struct parsing failed
        assert!(app_data.upnp_enabled);

        Ok(())
    }

    #[test]
    fn test_normal_parsing_unchanged() -> Result<()> {
        let temp_dir = tempdir()?;
        let config_path = temp_dir.path().join("valid.json");

        // Complete, valid config file
        let valid_app_data = AppData {
            rewards_address: Some(
                "0xabcdefabcdefabcdefabcdefabcdefabcdefabcd"
                    .parse()
                    .unwrap(),
            ),
            nodes_to_start: 3,
            storage_mountpoint: Some("/valid/path".into()),
            storage_drive: Some("D:".to_string()),
            upnp_enabled: false,
            port_range: Some((15000, 16000)),
        };
        valid_app_data.save(Some(config_path.clone()))?;

        let loaded_app_data = AppData::load(Some(config_path))?;

        // Should parse normally without any fallback/salvage
        assert_eq!(
            loaded_app_data.rewards_address,
            valid_app_data.rewards_address
        );
        assert_eq!(loaded_app_data.nodes_to_start, 3);
        assert!(!loaded_app_data.upnp_enabled);

        Ok(())
    }
}
