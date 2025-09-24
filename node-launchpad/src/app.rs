// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::components::popup::upgrade_launchpad::UpgradeLaunchpadPopup;
use crate::{
    action::Action,
    components::{
        Component,
        help::Help,
        options::Options,
        popup::{
            change_drive::ChangeDrivePopup, manage_nodes::ManageNodesPopup,
            node_logs::NodeLogsPopup, remove_node::RemoveNodePopUp, reset_nodes::ResetNodesPopup,
            rewards_address::RewardsAddressPopup, upgrade_nodes::UpgradeNodesPopUp,
        },
        status::{Status, StatusConfig},
    },
    config::{AppData, get_launchpad_nodes_data_dir_path},
    focus::{EventResult, FocusManager, FocusTarget},
    keybindings::KeyBindings,
    keybindings::get_keybindings,
    log_management::LogManagement,
    mode::{InputMode, Scene},
    node_management::NodeManagementHandle,
    node_stats::{AsyncMetricsFetcher, MetricsFetcher},
    runtime::{ProductionRuntime, Runtime},
    style::SPACE_CADET,
    system::{get_default_mount_point, get_primary_mount_point, get_primary_mount_point_name},
    tui,
};
use ant_bootstrap::InitialPeersConfig;
use ant_service_management::NodeRegistryManager;
use color_eyre::eyre::Result;
use crossterm::event::KeyEvent;
use ratatui::{prelude::Rect, style::Style, widgets::Block};
use std::{path::PathBuf, sync::Arc};
use tokio::sync::mpsc::{self, UnboundedSender};
use tracing::{debug, info};

pub struct App {
    pub keybindings: KeyBindings,
    pub app_data: AppData,
    pub tick_rate: f64,
    pub frame_rate: f64,
    pub components: Vec<Box<dyn Component>>,
    pub should_quit: bool,
    pub should_suspend: bool,
    pub input_mode: InputMode,
    pub scene: Scene,
    pub last_tick_key_events: Vec<KeyEvent>,
    pub focus_manager: FocusManager,
    persist_app_data: bool,
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
        Self::new_with_dependencies(
            tick_rate,
            frame_rate,
            init_peers_config,
            antnode_path,
            app_data_path,
            network_id,
            None,
            None,
            None,
            true,
            None,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn new_with_dependencies(
        tick_rate: f64,
        frame_rate: f64,
        init_peers_config: InitialPeersConfig,
        antnode_path: Option<PathBuf>,
        app_data_path: Option<PathBuf>,
        network_id: Option<u8>,
        node_management: Option<Arc<dyn NodeManagementHandle>>,
        node_registry_manager: Option<NodeRegistryManager>,
        app_data_override: Option<AppData>,
        persist_app_data: bool,
        metrics_fetcher: Option<Arc<dyn MetricsFetcher>>,
    ) -> Result<Self> {
        // Configurations
        let mut app_data = AppData::load(app_data_path)?;
        if let Some(custom_app_data) = app_data_override {
            app_data = custom_app_data;
        }
        let keybindings = get_keybindings();

        let metrics_fetcher: Arc<dyn MetricsFetcher> = metrics_fetcher.unwrap_or_else(|| {
            let fetcher: Arc<dyn MetricsFetcher> = Arc::new(AsyncMetricsFetcher);
            fetcher
        });

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
        let upnp_enabled = app_data.upnp_enabled;
        let port_range = app_data.port_range;
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
            rewards_address: app_data.rewards_address,
            init_peers_config,
            network_id,
            antnode_path,
            data_dir_path,
            upnp_enabled,
            port_range,
            storage_mountpoint: storage_mountpoint.clone(),
            node_management,
            node_registry_manager,
            metrics_fetcher: Arc::clone(&metrics_fetcher),
        };

        let status = Status::new(status_config).await?;
        let options = Options::new(
            storage_mountpoint.clone(),
            storage_drive.clone(),
            app_data.rewards_address,
            upnp_enabled,
            port_range,
        );
        let help = Help::new()?;

        // Popups
        let reset_nodes = ResetNodesPopup::default();
        let manage_nodes =
            ManageNodesPopup::new(app_data.nodes_to_start, storage_mountpoint.clone())?;
        let change_drive =
            ChangeDrivePopup::new(storage_mountpoint.clone(), app_data.nodes_to_start)?;
        let rewards_address = RewardsAddressPopup::new(app_data.rewards_address);
        let upgrade_nodes = UpgradeNodesPopUp::new();
        let remove_node = RemoveNodePopUp::default();
        let upgrade_launchpad_popup = UpgradeLaunchpadPopup::default();
        let node_logs = NodeLogsPopup::new(LogManagement::new()?);

        let components: Vec<Box<dyn Component>> = vec![
            // Sections
            Box::new(status),
            Box::new(options),
            Box::new(help),
            // Popups
            Box::new(change_drive),
            Box::new(rewards_address),
            Box::new(reset_nodes),
            Box::new(manage_nodes),
            Box::new(upgrade_nodes),
            Box::new(remove_node),
            Box::new(upgrade_launchpad_popup),
            Box::new(node_logs),
        ];

        Ok(Self {
            keybindings,
            app_data: AppData {
                rewards_address: app_data.rewards_address,
                nodes_to_start: app_data.nodes_to_start,
                storage_mountpoint: Some(storage_mountpoint),
                storage_drive: Some(storage_drive),
                upnp_enabled,
                port_range,
            },
            tick_rate,
            frame_rate,
            components,
            should_quit: false,
            should_suspend: false,
            input_mode: InputMode::Navigation,
            scene: Scene::Status,
            last_tick_key_events: Vec::new(),
            focus_manager: FocusManager::new(FocusTarget::Status), // Start with Status focused
            persist_app_data,
        })
    }

    fn is_popup_scene(&self, scene: Scene) -> bool {
        matches!(
            scene,
            Scene::ChangeDrivePopUp
                | Scene::StatusRewardsAddressPopUp
                | Scene::OptionsRewardsAddressPopUp
                | Scene::ManageNodesPopUp { .. }
                | Scene::ResetNodesPopUp
                | Scene::UpgradeNodesPopUp
                | Scene::UpgradeLaunchpadPopUp
                | Scene::RemoveNodePopUp
                | Scene::NodeLogsPopUp
        )
    }

    /// Handle a single key event and return the actions generated
    pub fn handle_key_event(
        &mut self,
        key: KeyEvent,
        action_tx: &UnboundedSender<Action>,
    ) -> Result<Vec<Action>> {
        let mut actions = Vec::new();

        if self.input_mode == InputMode::Navigation {
            let mut key_handled = false;
            if let Some(keymap) = self.keybindings.get(&self.scene) {
                if let Some(action) = keymap.get(&vec![key]) {
                    info!("Got action from keybindings {action:?}");
                    action_tx.send(action.clone())?;
                    actions.push(action.clone());
                    key_handled = true;
                } else {
                    // If the key was not handled as a single key action,
                    // then consider it for multi-key combinations.
                    self.last_tick_key_events.push(key);

                    // Check for multi-key combinations
                    if let Some(action) = keymap.get(&self.last_tick_key_events) {
                        info!("Got action from keybindings: {action:?}");
                        action_tx.send(action.clone())?;
                        actions.push(action.clone());
                        key_handled = true;
                    }
                }
            }

            // If no keybinding handled the key, let components handle it
            if !key_handled {
                for component in self.components.iter_mut() {
                    let (send_back_actions, event_result) =
                        component.handle_key_events(key, &self.focus_manager)?;
                    for action in &send_back_actions {
                        action_tx.send(action.clone())?;
                    }
                    actions.extend(send_back_actions);
                    // If the event was consumed, break to avoid other components handling it
                    if matches!(event_result, EventResult::Consumed) {
                        break;
                    }
                }
            }
        } else if self.input_mode == InputMode::Entry {
            for component in self.components.iter_mut() {
                let (send_back_actions, event_result) =
                    component.handle_key_events(key, &self.focus_manager)?;
                for action in &send_back_actions {
                    action_tx.send(action.clone())?;
                }
                actions.extend(send_back_actions);
                // If the event was consumed, break to avoid other components handling it
                if matches!(event_result, EventResult::Consumed) {
                    break;
                }
            }
        }

        Ok(actions)
    }

    /// Process a single action and sends the newly generated actions back through the channel.
    pub fn process_action(
        &mut self,
        action: Action,
        action_tx: &UnboundedSender<Action>,
    ) -> Result<()> {
        match action {
            Action::Tick => {
                self.last_tick_key_events.drain(..);
            }
            Action::Quit => self.should_quit = true,
            Action::Suspend => self.should_suspend = true,
            Action::Resume => self.should_suspend = false,
            Action::SwitchScene(scene) => {
                info!("Scene switched to: {scene:?}");
                let previous_scene = self.scene;
                self.scene = scene;

                // Handle focus transitions based on scene type
                match scene {
                    // Main scenes - set focus directly
                    Scene::Status => {
                        self.focus_manager.clear_and_set(FocusTarget::Status);
                    }
                    Scene::Options => {
                        self.focus_manager.clear_and_set(FocusTarget::Options);
                    }
                    Scene::Help => {
                        self.focus_manager.clear_and_set(FocusTarget::Help);
                    }
                    // Popup scenes - push focus to maintain stack
                    Scene::ChangeDrivePopUp => {
                        self.focus_manager.push_focus(FocusTarget::ChangeDrivePopup);
                    }
                    Scene::StatusRewardsAddressPopUp => {
                        self.focus_manager
                            .push_focus(FocusTarget::RewardsAddressPopup);
                    }
                    Scene::OptionsRewardsAddressPopUp => {
                        self.focus_manager
                            .push_focus(FocusTarget::RewardsAddressPopup);
                    }
                    Scene::ManageNodesPopUp { .. } => {
                        self.focus_manager.push_focus(FocusTarget::ManageNodesPopup);
                    }
                    Scene::ResetNodesPopUp => {
                        self.focus_manager.push_focus(FocusTarget::ResetNodesPopup);
                    }
                    Scene::UpgradeNodesPopUp => {
                        self.focus_manager
                            .push_focus(FocusTarget::UpgradeNodesPopup);
                    }
                    Scene::UpgradeLaunchpadPopUp => {
                        self.focus_manager
                            .push_focus(FocusTarget::UpgradeLaunchpadPopup);
                    }
                    Scene::RemoveNodePopUp => {
                        self.focus_manager.push_focus(FocusTarget::RemoveNodePopup);
                    }
                    Scene::NodeLogsPopUp => {
                        self.focus_manager.push_focus(FocusTarget::NodeLogsPopup);
                    }
                }

                // If we're closing a popup (going from popup to main scene), pop focus
                if self.is_popup_scene(previous_scene) && !self.is_popup_scene(scene) {
                    self.focus_manager.pop_focus();
                }
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
                if self.persist_app_data {
                    self.app_data.save(None)?;
                }
            }
            Action::StoreUpnpSetting(ref enabled) => {
                debug!("Storing UPnP setting: {enabled:?}");
                self.app_data.upnp_enabled = *enabled;
                if self.persist_app_data {
                    self.app_data.save(None)?;
                }
            }
            Action::StorePortRange(ref range) => {
                debug!("Storing port range: {range:?}");
                self.app_data.port_range = *range;
                if self.persist_app_data {
                    self.app_data.save(None)?;
                }
            }
            Action::StoreRewardsAddress(ref rewards_address) => {
                debug!("Storing rewards address: {rewards_address:?}");
                self.app_data.rewards_address = Some(*rewards_address);
                if self.persist_app_data {
                    self.app_data.save(None)?;
                }
            }
            Action::StoreRunningNodeCount(ref count) => {
                debug!("Storing nodes to start: {count:?}");
                self.app_data.nodes_to_start = *count;
                if self.persist_app_data {
                    self.app_data.save(None)?;
                }
            }
            _ => {}
        }

        // Always forward all actions to components so they can update their internal state
        for component in self.components.iter_mut() {
            if let Some(new_action) = component.update(action.clone())? {
                action_tx.send(new_action.clone())?;
            }
        }

        Ok(())
    }

    pub fn render_frame(
        &mut self,
        f: &mut ratatui::Frame,
        action_tx: &UnboundedSender<Action>,
    ) -> Result<()> {
        f.render_widget(Block::new().style(Style::new().bg(SPACE_CADET)), f.area());
        for component in self.components.iter_mut() {
            let should_draw = self
                .focus_manager
                .get_focus_stack()
                .contains(&component.focus_target());

            if should_draw && let Err(e) = component.draw(f, f.area()) {
                action_tx.send(Action::Error(format!("Failed to draw: {e:?}")))?;
            }
        }
        Ok(())
    }

    pub fn init_components(
        &mut self,
        rect: Rect,
        action_tx: UnboundedSender<Action>,
    ) -> Result<()> {
        for component in self.components.iter_mut() {
            component.register_action_handler(action_tx.clone())?;
            component.init(rect)?;
        }
        Ok(())
    }

    pub async fn run(&mut self) -> Result<()> {
        let mut runtime = ProductionRuntime::new(self.tick_rate, self.frame_rate)?;
        self.run_with_runtime(&mut runtime).await
    }

    pub async fn run_with_runtime<R: Runtime>(&mut self, runtime: &mut R) -> Result<()> {
        let (action_tx, mut action_rx) = mpsc::unbounded_channel();

        runtime.enter()?;
        let is_test_runtime = runtime
            .as_any_mut()
            .downcast_mut::<crate::runtime::TestRuntime>()
            .is_some();
        let size = runtime.size()?;
        self.init_components(size, action_tx.clone())?;

        loop {
            if let Some(e) = runtime.next_event().await {
                match e {
                    tui::Event::Quit => {
                        action_tx.send(Action::Quit)?;
                    }
                    tui::Event::Tick => action_tx.send(Action::Tick)?,
                    tui::Event::Render => action_tx.send(Action::Render)?,
                    tui::Event::Resize(x, y) => action_tx.send(Action::Resize(x, y))?,
                    tui::Event::Key(key) => {
                        self.handle_key_event(key, &action_tx)?;
                    }
                    _ => {}
                }
            }

            while let Ok(action) = action_rx.try_recv() {
                if action != Action::Tick && action != Action::Render {
                    debug!("{action:?}");
                }
                match action {
                    Action::Resize(w, h) => {
                        runtime.resize(Rect::new(0, 0, w, h))?;
                        runtime.draw(Box::new(|f| self.render_frame(f, &action_tx)))?;
                    }
                    Action::Render => {
                        runtime.draw(Box::new(|f| self.render_frame(f, &action_tx)))?;

                        // Check for pending test assertions after rendering
                        if is_test_runtime {
                            if let Some(test_runtime) = runtime
                                .as_any_mut()
                                .downcast_mut::<crate::runtime::TestRuntime>(
                            ) {
                                test_runtime.check_pending_assertion(self)?;
                            } else {
                                error!("Runtime is marked as test, but downcast failed");
                            }
                        }
                    }
                    // Use unified action processing for all other actions
                    _ => {
                        self.process_action(action.clone(), &action_tx)?;
                    }
                }
            }

            if self.should_suspend {
                runtime.suspend()?;
                action_tx.send(Action::Resume)?;
                runtime.enter()?;
            } else if self.should_quit {
                // In test mode, ensure all pending actions are processed before quitting
                if is_test_runtime {
                    debug!("Processing pending actions before quit");
                    let mut pending_actions = Vec::new();
                    while let Ok(action) = action_rx.try_recv() {
                        pending_actions.push(action);
                    }

                    // Process any remaining actions, especially render actions for assertions
                    for action in pending_actions {
                        if action != Action::Tick {
                            debug!("Processing final action before quit: {action:?}");
                        }
                        match action {
                            Action::Render => {
                                runtime.draw(Box::new(|f| self.render_frame(f, &action_tx)))?;
                                if let Some(test_runtime) = runtime
                                    .as_any_mut()
                                    .downcast_mut::<crate::runtime::TestRuntime>(
                                ) {
                                    test_runtime.check_pending_assertion(self)?;
                                }
                            }
                            _ => {
                                self.process_action(action, &action_tx)?;
                            }
                        }
                    }
                }

                runtime.stop()?;
                break;
            }
        }
        runtime.exit()?;
        info!("Exiting application");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ant_bootstrap::InitialPeersConfig;
    use color_eyre::eyre::Result;
    use std::io::Cursor;
    use std::io::Write;
    use tempfile::tempdir;

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
                assert_eq!(app.app_data.rewards_address, None);
                assert_eq!(app.app_data.nodes_to_start, 1);
                assert!(app.app_data.storage_mountpoint.is_some());
                assert!(app.app_data.storage_drive.is_some());
                assert!(app.app_data.upnp_enabled);
                assert_eq!(app.app_data.port_range, None);

                write!(
                    output,
                    "App created successfully with default configuration"
                )?;
            }
            Err(e) => {
                write!(output, "App creation failed: {e}")?;
            }
        }

        // Convert captured output to string
        let output_str = String::from_utf8(output.into_inner())?;

        // Check if the success message is in the output
        assert!(
            output_str.contains("App created successfully with default configuration"),
            "Unexpected output: {output_str}"
        );

        Ok(())
    }
}
