// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use std::path::PathBuf;

use crate::upnp::{get_upnp_support, UpnpSupport};
use crate::{
    action::Action,
    components::{
        help::Help,
        options::Options,
        popup::{
            change_drive::ChangeDrivePopup, connection_mode::ChangeConnectionModePopUp,
            manage_nodes::ManageNodes, port_range::PortRangePopUp, reset_nodes::ResetNodesPopup,
            rewards_address::RewardsAddress, upgrade_nodes::UpgradeNodesPopUp,
        },
        status::{Status, StatusConfig},
        Component,
    },
    config::{get_launchpad_nodes_data_dir_path, AppData, Config},
    connection_mode::ConnectionMode,
    mode::{InputMode, Scene},
    node_mgmt::{PORT_MAX, PORT_MIN},
    style::SPACE_CADET,
    system::{get_default_mount_point, get_primary_mount_point, get_primary_mount_point_name},
    tui,
};
use ant_bootstrap::InitialPeersConfig;
use color_eyre::eyre::Result;
use crossterm::event::KeyEvent;
use ratatui::{prelude::Rect, style::Style, widgets::Block};
use tokio::sync::mpsc;

pub struct App {
    pub config: Config,
    pub app_data: AppData,
    pub tick_rate: f64,
    pub frame_rate: f64,
    pub components: Vec<Box<dyn Component>>,
    pub should_quit: bool,
    pub should_suspend: bool,
    pub input_mode: InputMode,
    pub scene: Scene,
    pub last_tick_key_events: Vec<KeyEvent>,
}

impl App {
    pub async fn new(
        tick_rate: f64,
        frame_rate: f64,
        init_peers_config: InitialPeersConfig,
        antnode_path: Option<PathBuf>,
        app_data_path: Option<PathBuf>,
        network_id: Option<u8>,
    ) -> Result<Self> {
        // Configurations
        let app_data = AppData::load(app_data_path)?;
        let config = Config::new()?;

        // Tries to set the data dir path based on the storage mountpoint set by the user,
        // if not set, it tries to get the default mount point (where the executable is) and
        // create the nodes data dir there.
        // If even that fails, it will create the nodes data dir in the primary mount point.
        let data_dir_path = match &app_data.storage_mountpoint {
            Some(path) => get_launchpad_nodes_data_dir_path(&PathBuf::from(path), true)?,
            None => match get_default_mount_point() {
                Ok((_, path)) => get_launchpad_nodes_data_dir_path(&path, true)?,
                Err(_) => get_launchpad_nodes_data_dir_path(&get_primary_mount_point(), true)?,
            },
        };
        debug!("Data dir path for nodes: {data_dir_path:?}");

        // App data default values
        let connection_mode = app_data
            .connection_mode
            .unwrap_or(ConnectionMode::Automatic);

        let upnp_support = UpnpSupport::Loading;

        let port_from = app_data.port_from.unwrap_or(PORT_MIN);
        let port_to = app_data.port_to.unwrap_or(PORT_MAX);
        let storage_mountpoint = app_data
            .storage_mountpoint
            .clone()
            .unwrap_or(get_primary_mount_point());
        let storage_drive = app_data
            .storage_drive
            .clone()
            .unwrap_or(get_primary_mount_point_name()?);

        // Main Screens
        let status_config = StatusConfig {
            allocated_disk_space: app_data.nodes_to_start,
            rewards_address: app_data.discord_username.clone(),
            init_peers_config,
            network_id,
            antnode_path,
            data_dir_path,
            connection_mode,
            upnp_support,
            port_from: Some(port_from),
            port_to: Some(port_to),
        };

        let status = Status::new(status_config).await?;
        let options = Options::new(
            storage_mountpoint.clone(),
            storage_drive.clone(),
            app_data.discord_username.clone(),
            connection_mode,
            Some(port_from),
            Some(port_to),
        )
        .await?;
        let help = Help::new().await?;

        // Popups
        let reset_nodes = ResetNodesPopup::default();
        let manage_nodes = ManageNodes::new(app_data.nodes_to_start, storage_mountpoint.clone())?;
        let change_drive =
            ChangeDrivePopup::new(storage_mountpoint.clone(), app_data.nodes_to_start)?;
        let change_connection_mode = ChangeConnectionModePopUp::new(connection_mode)?;
        let port_range = PortRangePopUp::new(connection_mode, port_from, port_to);
        let rewards_address = RewardsAddress::new(app_data.discord_username.clone());
        let upgrade_nodes = UpgradeNodesPopUp::new(app_data.nodes_to_start);

        Ok(Self {
            config,
            app_data: AppData {
                discord_username: app_data.discord_username.clone(),
                nodes_to_start: app_data.nodes_to_start,
                storage_mountpoint: Some(storage_mountpoint),
                storage_drive: Some(storage_drive),
                connection_mode: Some(connection_mode),
                port_from: Some(port_from),
                port_to: Some(port_to),
            },
            tick_rate,
            frame_rate,
            components: vec![
                // Sections
                Box::new(status),
                Box::new(options),
                Box::new(help),
                // Popups
                Box::new(change_drive),
                Box::new(change_connection_mode),
                Box::new(port_range),
                Box::new(rewards_address),
                Box::new(reset_nodes),
                Box::new(manage_nodes),
                Box::new(upgrade_nodes),
            ],
            should_quit: false,
            should_suspend: false,
            input_mode: InputMode::Navigation,
            scene: Scene::Status,
            last_tick_key_events: Vec::new(),
        })
    }

    pub async fn run(&mut self) -> Result<()> {
        let (action_tx, mut action_rx) = mpsc::unbounded_channel();

        let action_tx_clone = action_tx.clone();

        tokio::spawn(async move {
            let upnp_support = tokio::task::spawn_blocking(get_upnp_support)
                .await
                .unwrap_or(UpnpSupport::Unknown);

            let _ = action_tx_clone.send(Action::SetUpnpSupport(upnp_support));
        });

        let mut tui = tui::Tui::new()?
            .tick_rate(self.tick_rate)
            .frame_rate(self.frame_rate);
        // tui.mouse(true);
        tui.enter()?;

        for component in self.components.iter_mut() {
            component.register_action_handler(action_tx.clone())?;
            component.register_config_handler(self.config.clone())?;
            let size = tui.size()?;
            let rect = Rect::new(0, 0, size.width, size.height);
            component.init(rect)?;
        }

        loop {
            if let Some(e) = tui.next().await {
                match e {
                    tui::Event::Quit => action_tx.send(Action::Quit)?,
                    tui::Event::Tick => action_tx.send(Action::Tick)?,
                    tui::Event::Render => action_tx.send(Action::Render)?,
                    tui::Event::Resize(x, y) => action_tx.send(Action::Resize(x, y))?,
                    tui::Event::Key(key) => {
                        if self.input_mode == InputMode::Navigation {
                            if let Some(keymap) = self.config.keybindings.get(&self.scene) {
                                if let Some(action) = keymap.get(&vec![key]) {
                                    info!("Got action: {action:?}");
                                    action_tx.send(action.clone())?;
                                } else {
                                    // If the key was not handled as a single key action,
                                    // then consider it for multi-key combinations.
                                    self.last_tick_key_events.push(key);

                                    // Check for multi-key combinations
                                    if let Some(action) = keymap.get(&self.last_tick_key_events) {
                                        info!("Got action: {action:?}");
                                        action_tx.send(action.clone())?;
                                    }
                                }
                            };
                        } else if self.input_mode == InputMode::Entry {
                            for component in self.components.iter_mut() {
                                let send_back_actions = component.handle_events(Some(e.clone()))?;
                                for action in send_back_actions {
                                    action_tx.send(action)?;
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }

            while let Ok(action) = action_rx.try_recv() {
                if action != Action::Tick && action != Action::Render {
                    debug!("{action:?}");
                }
                match action {
                    Action::Tick => {
                        self.last_tick_key_events.drain(..);
                    }
                    Action::Quit => self.should_quit = true,
                    Action::Suspend => self.should_suspend = true,
                    Action::Resume => self.should_suspend = false,
                    Action::Resize(w, h) => {
                        tui.resize(Rect::new(0, 0, w, h))?;
                        tui.draw(|f| {
                            for component in self.components.iter_mut() {
                                let r = component.draw(f, f.area());
                                if let Err(e) = r {
                                    action_tx
                                        .send(Action::Error(format!("Failed to draw: {:?}", e)))
                                        .unwrap();
                                }
                            }
                        })?;
                    }
                    Action::Render => {
                        tui.draw(|f| {
                            f.render_widget(
                                Block::new().style(Style::new().bg(SPACE_CADET)),
                                f.area(),
                            );
                            for component in self.components.iter_mut() {
                                let r = component.draw(f, f.area());
                                if let Err(e) = r {
                                    action_tx
                                        .send(Action::Error(format!("Failed to draw: {:?}", e)))
                                        .unwrap();
                                }
                            }
                        })?;
                    }
                    Action::SwitchScene(scene) => {
                        info!("Scene switched to: {scene:?}");
                        self.scene = scene;
                    }
                    Action::SwitchInputMode(mode) => {
                        info!("Input mode switched to: {mode:?}");
                        self.input_mode = mode;
                    }
                    // Storing Application Data
                    Action::StoreStorageDrive(ref drive_mountpoint, ref drive_name) => {
                        debug!("Storing storage drive: {drive_mountpoint:?}, {drive_name:?}");
                        self.app_data.storage_mountpoint = Some(drive_mountpoint.clone());
                        self.app_data.storage_drive = Some(drive_name.as_str().to_string());
                        self.app_data.save(None)?;
                    }
                    Action::StoreConnectionMode(ref mode) => {
                        debug!("Storing connection mode: {mode:?}");
                        self.app_data.connection_mode = Some(*mode);
                        self.app_data.save(None)?;
                    }
                    Action::StorePortRange(ref from, ref to) => {
                        debug!("Storing port range: {from:?}, {to:?}");
                        self.app_data.port_from = Some(*from);
                        self.app_data.port_to = Some(*to);
                        self.app_data.save(None)?;
                    }
                    Action::StoreRewardsAddress(ref rewards_address) => {
                        debug!("Storing rewards address: {rewards_address:?}");
                        self.app_data.discord_username.clone_from(rewards_address);
                        self.app_data.save(None)?;
                    }
                    Action::StoreNodesToStart(ref count) => {
                        debug!("Storing nodes to start: {count:?}");
                        self.app_data.nodes_to_start = *count;
                        self.app_data.save(None)?;
                    }
                    _ => {}
                }
                for component in self.components.iter_mut() {
                    if let Some(action) = component.update(action.clone())? {
                        action_tx.send(action)?
                    };
                }
            }
            if self.should_suspend {
                tui.suspend()?;
                action_tx.send(Action::Resume)?;
                tui = tui::Tui::new()?
                    .tick_rate(self.tick_rate)
                    .frame_rate(self.frame_rate);
                // tui.mouse(true);
                tui.enter()?;
            } else if self.should_quit {
                tui.stop()?;
                break;
            }
        }
        tui.exit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ant_bootstrap::InitialPeersConfig;
    use color_eyre::eyre::Result;
    use serde_json::json;
    use std::io::Cursor;
    use std::io::Write;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_app_creation_with_valid_config() -> Result<()> {
        // Create a temporary directory for our test
        let temp_dir = tempdir()?;
        let config_path = temp_dir.path().join("valid_config.json");

        let mountpoint = get_primary_mount_point();

        let config = json!({
            "discord_username": "happy_user",
            "nodes_to_start": 5,
            "storage_mountpoint": mountpoint.display().to_string(),
            "storage_drive": "C:",
            "connection_mode": "Automatic",
            "port_from": 12000,
            "port_to": 13000
        });

        let valid_config = serde_json::to_string_pretty(&config)?;
        std::fs::write(&config_path, valid_config)?;

        // Create default PeersArgs
        let init_peers_config = InitialPeersConfig::default();

        // Create a buffer to capture output
        let mut output = Cursor::new(Vec::new());

        // Create and run the App, capturing its output
        let app_result =
            App::new(60.0, 60.0, init_peers_config, None, Some(config_path), None).await;

        match app_result {
            Ok(app) => {
                // Check if all fields were correctly loaded
                assert_eq!(app.app_data.discord_username, "happy_user");
                assert_eq!(app.app_data.nodes_to_start, 5);
                assert_eq!(app.app_data.storage_mountpoint, Some(mountpoint));
                assert_eq!(app.app_data.storage_drive, Some("C:".to_string()));
                assert_eq!(
                    app.app_data.connection_mode,
                    Some(ConnectionMode::Automatic)
                );
                assert_eq!(app.app_data.port_from, Some(12000));
                assert_eq!(app.app_data.port_to, Some(13000));

                write!(output, "App created successfully with valid configuration")?;
            }
            Err(e) => {
                write!(output, "App creation failed: {}", e)?;
            }
        }

        // Convert captured output to string
        let output_str = String::from_utf8(output.into_inner())?;

        // Check if the success message is in the output
        assert!(
            output_str.contains("App created successfully with valid configuration"),
            "Unexpected output: {}",
            output_str
        );

        Ok(())
    }

    #[tokio::test]
    async fn test_app_should_run_when_storage_mountpoint_not_set() -> Result<()> {
        // Create a temporary directory for our test
        let temp_dir = tempdir()?;
        let test_app_data_path = temp_dir.path().join("test_app_data.json");

        // Create a custom configuration file with only some settings
        let custom_config = r#"
        {
            "discord_username": "test_user",
            "nodes_to_start": 3,
            "connection_mode": "Custom Ports",
            "port_from": 12000,
            "port_to": 13000
        }
        "#;
        std::fs::write(&test_app_data_path, custom_config)?;

        // Create default PeersArgs
        let init_peers_config = InitialPeersConfig::default();

        // Create a buffer to capture output
        let mut output = Cursor::new(Vec::new());

        // Create and run the App, capturing its output
        let app_result = App::new(
            60.0,
            60.0,
            init_peers_config,
            None,
            Some(test_app_data_path),
            None,
        )
        .await;

        match app_result {
            Ok(app) => {
                // Check if the fields were correctly loaded
                assert_eq!(app.app_data.discord_username, "test_user");
                assert_eq!(app.app_data.nodes_to_start, 3);
                // Check if the storage_mountpoint is Some (automatically set)
                assert!(app.app_data.storage_mountpoint.is_some());
                // Check if the storage_drive is Some (automatically set)
                assert!(app.app_data.storage_drive.is_some());
                // Check the new fields
                assert_eq!(
                    app.app_data.connection_mode,
                    Some(ConnectionMode::CustomPorts)
                );
                assert_eq!(app.app_data.port_from, Some(12000));
                assert_eq!(app.app_data.port_to, Some(13000));

                write!(
                    output,
                    "App created successfully with partial configuration"
                )?;
            }
            Err(e) => {
                write!(output, "App creation failed: {}", e)?;
            }
        }

        // Convert captured output to string
        let output_str = String::from_utf8(output.into_inner())?;

        // Check if the success message is in the output
        assert!(
            output_str.contains("App created successfully with partial configuration"),
            "Unexpected output: {}",
            output_str
        );

        Ok(())
    }

    #[tokio::test]
    async fn test_app_creation_when_config_file_doesnt_exist() -> Result<()> {
        // Create a temporary directory for our test
        let temp_dir = tempdir()?;
        let non_existent_config_path = temp_dir.path().join("non_existent_config.json");

        // Create default PeersArgs
        let init_peers_config = InitialPeersConfig::default();

        // Create a buffer to capture output
        let mut output = Cursor::new(Vec::new());

        // Create and run the App, capturing its output
        let app_result = App::new(
            60.0,
            60.0,
            init_peers_config,
            None,
            Some(non_existent_config_path),
            None,
        )
        .await;

        match app_result {
            Ok(app) => {
                assert_eq!(app.app_data.discord_username, "");
                assert_eq!(app.app_data.nodes_to_start, 1);
                assert!(app.app_data.storage_mountpoint.is_some());
                assert!(app.app_data.storage_drive.is_some());
                assert_eq!(
                    app.app_data.connection_mode,
                    Some(ConnectionMode::Automatic)
                );
                assert_eq!(app.app_data.port_from, Some(PORT_MIN));
                assert_eq!(app.app_data.port_to, Some(PORT_MAX));

                write!(
                    output,
                    "App created successfully with default configuration"
                )?;
            }
            Err(e) => {
                write!(output, "App creation failed: {}", e)?;
            }
        }

        // Convert captured output to string
        let output_str = String::from_utf8(output.into_inner())?;

        // Check if the success message is in the output
        assert!(
            output_str.contains("App created successfully with default configuration"),
            "Unexpected output: {}",
            output_str
        );

        Ok(())
    }

    #[tokio::test]
    async fn test_app_creation_with_invalid_storage_mountpoint() -> Result<()> {
        // Create a temporary directory for our test
        let temp_dir = tempdir()?;
        let config_path = temp_dir.path().join("invalid_config.json");

        // Create a configuration file with an invalid storage_mountpoint
        let invalid_config = r#"
        {
            "discord_username": "test_user",
            "nodes_to_start": 5,
            "storage_mountpoint": "/non/existent/path",
            "storage_drive": "Z:",
            "connection_mode": "Custom Ports",
            "port_from": 12000,
            "port_to": 13000
        }
        "#;
        std::fs::write(&config_path, invalid_config)?;

        // Create default PeersArgs
        let init_peers_config = InitialPeersConfig::default();

        // Create and run the App, capturing its output
        let app_result =
            App::new(60.0, 60.0, init_peers_config, None, Some(config_path), None).await;

        // Could be that the mountpoint doesn't exists
        // or that the user doesn't have permissions to access it
        match app_result {
            Ok(_) => {
                panic!("App creation should have failed due to invalid storage_mountpoint");
            }
            Err(e) => {
                assert!(
                    e.to_string().contains(
                        "Cannot find the primary disk. Configuration file might be wrong."
                    ) || e.to_string().contains("Failed to create nodes data dir in"),
                    "Unexpected error message: {}",
                    e
                );
            }
        }

        Ok(())
    }

    #[tokio::test]
    async fn test_app_default_connection_mode_and_ports() -> Result<()> {
        // Create a temporary directory for our test
        let temp_dir = tempdir()?;
        let test_app_data_path = temp_dir.path().join("test_app_data.json");

        // Create a custom configuration file without connection mode and ports
        let custom_config = r#"
        {
            "discord_username": "test_user",
            "nodes_to_start": 3
        }
        "#;
        std::fs::write(&test_app_data_path, custom_config)?;

        // Create default PeersArgs
        let init_peers_config = InitialPeersConfig::default();

        // Create and run the App
        let app_result = App::new(
            60.0,
            60.0,
            init_peers_config,
            None,
            Some(test_app_data_path),
            None,
        )
        .await;

        match app_result {
            Ok(app) => {
                // Check if the discord_username and nodes_to_start were correctly loaded
                assert_eq!(app.app_data.discord_username, "test_user");
                assert_eq!(app.app_data.nodes_to_start, 3);

                // Check if the connection_mode is set to the default (Automatic)
                assert_eq!(
                    app.app_data.connection_mode,
                    Some(ConnectionMode::Automatic)
                );

                // Check if the port range is set to the default values
                assert_eq!(app.app_data.port_from, Some(PORT_MIN));
                assert_eq!(app.app_data.port_to, Some(PORT_MAX));

                println!("App created successfully with default connection mode and ports");
            }
            Err(e) => {
                panic!("App creation failed: {}", e);
            }
        }

        Ok(())
    }
}
