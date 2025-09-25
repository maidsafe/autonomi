// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::action::{NodeManagementCommand, NodeTableActions, OptionsActions};
use crate::system::get_available_space_b;
use color_eyre::Result;
use crossterm::event::{Event, KeyCode, KeyEvent};
use ratatui::{prelude::*, widgets::*};
use std::{any::Any, path::PathBuf};
use tui_input::{Input, backend::crossterm::EventHandler};

use crate::{
    action::Action,
    focus::{EventResult, FocusManager, FocusTarget},
    mode::{InputMode, Scene},
    style::{EUCALYPTUS, GHOST_WHITE, LIGHT_PERIWINKLE, VIVID_SKY_BLUE, clear_area},
};

use super::super::{Component, utils::centered_rect_fixed};

pub const GB_PER_NODE: u64 = 35;
pub const MB: u64 = 1000 * 1000;
pub const GB: u64 = MB * 1000;
pub const MAX_NODE_COUNT: u64 = 50;

pub struct ManageNodesPopup {
    available_disk_space_gb: u64,
    storage_mountpoint: PathBuf,
    nodes_to_start_input: Input,
    // cache the old value incase user presses Esc.
    old_value: String,
}

impl ManageNodesPopup {
    pub fn new(nodes_to_start: u64, storage_mountpoint: PathBuf) -> Result<Self> {
        let nodes_to_start = std::cmp::min(nodes_to_start, MAX_NODE_COUNT);
        let new = Self {
            available_disk_space_gb: get_available_space_b(storage_mountpoint.as_path())? / GB,
            nodes_to_start_input: Input::default().with_value(nodes_to_start.to_string()),
            old_value: Default::default(),
            storage_mountpoint: storage_mountpoint.clone(),
        };
        Ok(new)
    }

    /// Override the cached disk availability value, primarily for tests.
    /// Pair with `TestAppBuilder::with_available_disk_space` to bypass host-specific limits.
    pub(crate) fn override_available_disk_space(&mut self, gb: u64) {
        self.available_disk_space_gb = gb;
    }

    fn get_nodes_to_start_val(&self) -> u64 {
        self.nodes_to_start_input.value().parse().unwrap_or(0)
    }

    // Returns the max number of nodes to start
    // It is the minimum of the available disk space and the max nodes limit
    fn max_nodes_to_start(&self) -> u64 {
        std::cmp::min(self.available_disk_space_gb / GB_PER_NODE, MAX_NODE_COUNT)
    }

    fn handle_key_events_internal(&mut self, key: KeyEvent) -> Result<Vec<Action>> {
        // while in entry mode, key bindings are not captured, so gotta exit entry mode from here
        let send_back = match key.code {
            KeyCode::Enter => {
                let nodes_to_start_str = self.nodes_to_start_input.value().to_string();
                let requested_nodes = self.get_nodes_to_start_val();
                let nodes_to_start = std::cmp::min(requested_nodes, self.max_nodes_to_start());

                // set the new value
                self.nodes_to_start_input = self
                    .nodes_to_start_input
                    .clone()
                    .with_value(nodes_to_start.to_string());

                if requested_nodes > self.max_nodes_to_start() {
                    debug!(
                        "Requested {requested_nodes} node(s) but the limit is {limit}. Rejecting action.",
                        limit = self.max_nodes_to_start()
                    );
                    return Ok(vec![]);
                }

                if nodes_to_start == 0
                    && requested_nodes > 0
                    && self.available_disk_space_gb < GB_PER_NODE
                {
                    debug!(
                        "Requested {requested_nodes} node(s) but only {available}GB available. Rejecting action.",
                        available = self.available_disk_space_gb
                    );
                    return Ok(vec![]);
                }

                debug!(
                    "Got Enter, value found to be {nodes_to_start} derived from input: {nodes_to_start_str:?} and switching scene",
                );
                vec![
                    Action::StoreRunningNodeCount(nodes_to_start),
                    // this has to come after storing the new count
                    Action::NodeTableActions(NodeTableActions::NodeManagementCommand(
                        NodeManagementCommand::MaintainNodes,
                    )),
                    Action::SwitchScene(Scene::Status),
                ]
            }
            KeyCode::Esc => {
                debug!(
                    "Got Esc, restoring the old value {} and switching to home",
                    self.old_value
                );
                // reset to old value
                self.nodes_to_start_input = self
                    .nodes_to_start_input
                    .clone()
                    .with_value(self.old_value.clone());
                vec![Action::SwitchScene(Scene::Status)]
            }
            KeyCode::Char(c) if c.is_numeric() => {
                // don't allow leading zeros
                if c == '0' && self.nodes_to_start_input.value().is_empty() {
                    return Ok(vec![]);
                }
                let number = c.to_string().parse::<u64>().unwrap_or(0);
                let new_value = format!("{}{}", self.get_nodes_to_start_val(), number)
                    .parse::<u64>()
                    .unwrap_or(0);
                // if it might exceed the available space or if more than max_node_count, then enter the max
                if new_value * GB_PER_NODE > self.available_disk_space_gb * GB
                    || new_value > MAX_NODE_COUNT
                {
                    self.nodes_to_start_input = self
                        .nodes_to_start_input
                        .clone()
                        .with_value(self.max_nodes_to_start().to_string());
                    return Ok(vec![]);
                }
                self.nodes_to_start_input.handle_event(&Event::Key(key));
                vec![]
            }
            KeyCode::Backspace => {
                self.nodes_to_start_input.handle_event(&Event::Key(key));
                vec![]
            }
            KeyCode::Up | KeyCode::Down => {
                let nodes_to_start = {
                    let current_val = self.get_nodes_to_start_val();

                    if key.code == KeyCode::Up {
                        if current_val + 1 >= MAX_NODE_COUNT {
                            MAX_NODE_COUNT
                        } else if (current_val + 1) * GB_PER_NODE <= self.available_disk_space_gb {
                            current_val + 1
                        } else {
                            current_val
                        }
                    } else {
                        // Key::Down
                        if current_val == 0 { 0 } else { current_val - 1 }
                    }
                };
                // set the new value
                self.nodes_to_start_input = self
                    .nodes_to_start_input
                    .clone()
                    .with_value(nodes_to_start.to_string());
                vec![]
            }
            _ => {
                vec![]
            }
        };
        Ok(send_back)
    }
}

impl Component for ManageNodesPopup {
    fn handle_key_events(
        &mut self,
        key: KeyEvent,
        focus_manager: &FocusManager,
    ) -> Result<(Vec<Action>, EventResult)> {
        if !focus_manager.has_focus(&self.focus_target()) {
            return Ok((vec![], EventResult::Ignored));
        }

        let actions = self.handle_key_events_internal(key)?;
        let result = if actions.is_empty() {
            EventResult::Ignored
        } else {
            EventResult::Consumed
        };
        Ok((actions, result))
    }

    fn focus_target(&self) -> FocusTarget {
        FocusTarget::ManageNodesPopup
    }

    fn update(&mut self, action: Action) -> Result<Option<Action>> {
        let send_back = match action {
            Action::SwitchScene(Scene::ManageNodesPopUp { amount_of_nodes }) => {
                self.nodes_to_start_input = self
                    .nodes_to_start_input
                    .clone()
                    .with_value(amount_of_nodes.to_string());
                self.old_value = self.nodes_to_start_input.value().to_string();
                // set to entry input mode as we want to handle everything within our handle_key_events
                // so by default if this scene is active, we capture inputs.
                Some(Action::SwitchInputMode(InputMode::Entry))
            }
            Action::OptionsActions(OptionsActions::UpdateStorageDrive(mountpoint, _drive_name)) => {
                self.storage_mountpoint.clone_from(&mountpoint);
                self.available_disk_space_gb = get_available_space_b(mountpoint.as_path())? / GB;
                None
            }
            _ => None,
        };
        Ok(send_back)
    }

    fn draw(&mut self, f: &mut crate::tui::Frame<'_>, area: Rect) -> Result<()> {
        let layer_zero = centered_rect_fixed(52, 15, area);
        let layer_one = Layout::new(
            Direction::Vertical,
            [
                // for the pop_up_border
                Constraint::Length(2),
                // for the input field
                Constraint::Length(1),
                // for the info field telling how much gb used
                Constraint::Length(1),
                // gap before help
                Constraint::Length(1),
                // for the help
                Constraint::Length(7),
                // for the dash
                Constraint::Min(1),
                // for the buttons
                Constraint::Length(1),
                // for the pop_up_border
                Constraint::Length(1),
            ],
        )
        .split(layer_zero);
        let pop_up_border = Paragraph::new("").block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Manage Nodes ")
                .bold()
                .title_style(Style::new().fg(GHOST_WHITE))
                .title_style(Style::new().fg(EUCALYPTUS))
                .padding(Padding::uniform(2))
                .border_style(Style::new().fg(EUCALYPTUS)),
        );
        clear_area(f, layer_zero);

        // ==== input field ====
        let layer_input_field = Layout::new(
            Direction::Horizontal,
            [
                // for the gap
                Constraint::Min(5),
                // Start
                Constraint::Length(5),
                // Input box
                Constraint::Length(5),
                // Nodes(s)
                Constraint::Length(8),
                // gap
                Constraint::Min(5),
            ],
        )
        .split(layer_one[1]);

        let start = Paragraph::new("Start ").style(Style::default().fg(GHOST_WHITE));
        f.render_widget(start, layer_input_field[1]);

        let width = layer_input_field[2].width.max(3) - 3;
        let scroll = self.nodes_to_start_input.visual_scroll(width as usize);
        let input = Paragraph::new(self.get_nodes_to_start_val().to_string())
            .style(Style::new().fg(VIVID_SKY_BLUE))
            .scroll((0, scroll as u16))
            .alignment(Alignment::Center);

        f.render_widget(input, layer_input_field[2]);

        let nodes_text = Paragraph::new("Node(s)").fg(GHOST_WHITE);
        f.render_widget(nodes_text, layer_input_field[3]);

        // ==== info field ====
        let available_space_gb = self.available_disk_space_gb;
        let info_style = Style::default().fg(VIVID_SKY_BLUE);
        let info = Line::from(vec![
            Span::styled("Using", info_style),
            Span::styled(
                format!(" {}GB ", self.get_nodes_to_start_val() * GB_PER_NODE),
                info_style.bold(),
            ),
            Span::styled(
                format!("of {available_space_gb}GB available space"),
                info_style,
            ),
        ]);
        let info = Paragraph::new(info).alignment(Alignment::Center);
        f.render_widget(info, layer_one[2]);

        // ==== help ====
        let help = Paragraph::new(vec![
            Line::raw(format!(
                "Note: Each node will use a small amount of CPU Memory and Network Bandwidth. \
                 We recommend starting no more than 2 at a time (max {MAX_NODE_COUNT} nodes)."
            )),
            Line::raw(""),
            Line::raw("▲▼ to change the number of nodes to start."),
        ])
        .wrap(Wrap { trim: false })
        .block(Block::default().padding(Padding::horizontal(4)))
        .alignment(Alignment::Center)
        .fg(GHOST_WHITE);
        f.render_widget(help, layer_one[4]);

        // ==== dash ====
        let dash = Block::new()
            .borders(Borders::BOTTOM)
            .border_style(Style::new().fg(GHOST_WHITE));
        f.render_widget(dash, layer_one[5]);

        // ==== buttons ====
        let buttons_layer =
            Layout::horizontal(vec![Constraint::Percentage(45), Constraint::Percentage(55)])
                .split(layer_one[6]);

        let button_no = Line::from(vec![Span::styled(
            "  Close [Esc]",
            Style::default().fg(LIGHT_PERIWINKLE),
        )]);
        f.render_widget(button_no, buttons_layer[0]);
        let button_yes = Line::from(vec![Span::styled(
            "Start Node(s) [Enter]  ",
            Style::default().fg(EUCALYPTUS),
        )]);
        let button_yes = Paragraph::new(button_yes).alignment(Alignment::Right);
        f.render_widget(button_yes, buttons_layer[1]);

        f.render_widget(pop_up_border, layer_zero);

        Ok(())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::focus::FocusManager;
    use crossterm::event::KeyModifiers;
    use tempfile::TempDir;
    use tempfile::tempdir;

    fn build_popup(initial: u64) -> (TempDir, ManageNodesPopup) {
        let temp_dir = tempdir().expect("failed to create temp directory");
        let mount = temp_dir.path().to_path_buf();
        std::fs::create_dir_all(&mount).expect("failed to create temp directory");
        (
            temp_dir,
            ManageNodesPopup::new(initial, mount).expect("popup initialised"),
        )
    }

    #[test]
    fn numeric_input_clamps_to_max_node_count() {
        let (_temp_dir, mut popup) = build_popup(0);
        popup.available_disk_space_gb = GB_PER_NODE * (MAX_NODE_COUNT + 5);
        popup.nodes_to_start_input = Input::default();

        let _ = popup
            .handle_key_events_internal(KeyEvent::new(KeyCode::Char('5'), KeyModifiers::empty()))
            .expect("first digit");
        let _ = popup
            .handle_key_events_internal(KeyEvent::new(KeyCode::Char('9'), KeyModifiers::empty()))
            .expect("second digit");

        assert_eq!(
            popup.nodes_to_start_input.value(),
            MAX_NODE_COUNT.to_string()
        );
    }

    #[test]
    fn enter_confirms_value_and_dispatches_actions() {
        let (_temp_dir, mut popup) = build_popup(2);
        popup.available_disk_space_gb = GB_PER_NODE * 5;
        let actions = popup
            .handle_key_events_internal(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()))
            .expect("enter handled");

        assert!(
            actions
                .iter()
                .any(|a| matches!(a, Action::StoreRunningNodeCount(2)))
        );
        assert!(actions.iter().any(|a| matches!(
            a,
            Action::NodeTableActions(NodeTableActions::NodeManagementCommand(
                NodeManagementCommand::MaintainNodes
            ))
        )));
        assert!(
            actions
                .iter()
                .any(|a| matches!(a, Action::SwitchScene(Scene::Status)))
        );
    }

    #[test]
    fn update_switch_scene_enters_entry_mode() {
        let (_temp_dir, mut popup) = build_popup(3);
        let action = popup
            .update(Action::SwitchScene(Scene::ManageNodesPopUp {
                amount_of_nodes: 8,
            }))
            .expect("update processed")
            .expect("action produced");
        assert_eq!(action, Action::SwitchInputMode(InputMode::Entry));
        assert_eq!(popup.nodes_to_start_input.value(), "8");
        assert_eq!(popup.old_value, "8");
    }

    #[test]
    fn handle_key_events_requires_focus() {
        let (_temp_dir, mut popup) = build_popup(1);
        let focus_manager = FocusManager::new(FocusTarget::Status);
        let (actions, result) = popup
            .handle_key_events(
                KeyEvent::new(KeyCode::Char('1'), KeyModifiers::empty()),
                &focus_manager,
            )
            .expect("handled");
        assert!(actions.is_empty());
        assert_eq!(result, EventResult::Ignored);
    }

    #[test]
    fn boundary_conditions_disk_space_and_node_limits() {
        let (_temp_dir, mut popup) = build_popup(0);

        // Test with exact maximum nodes allowed by disk space
        popup.available_disk_space_gb = GB_PER_NODE * MAX_NODE_COUNT;
        popup.nodes_to_start_input = Input::default().with_value(MAX_NODE_COUNT.to_string());
        let actions = popup
            .handle_key_events_internal(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()))
            .expect("max nodes accepted");
        assert!(!actions.is_empty(), "should accept maximum allowable nodes");

        // Test insufficient disk space for single node
        popup.available_disk_space_gb = GB_PER_NODE / 2;
        popup.nodes_to_start_input = Input::default().with_value("1".to_string());
        let actions = popup
            .handle_key_events_internal(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()))
            .expect("insufficient space handled");
        assert!(
            actions.is_empty(),
            "should reject nodes when insufficient disk space"
        );

        // Test zero nodes
        popup.available_disk_space_gb = GB_PER_NODE * 10;
        popup.nodes_to_start_input = Input::default().with_value("0".to_string());
        let actions = popup
            .handle_key_events_internal(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()))
            .expect("zero nodes handled");
        assert!(!actions.is_empty(), "should accept zero nodes (removal)");

        // Test exactly at disk space boundary
        popup.available_disk_space_gb = GB_PER_NODE * 3;
        popup.nodes_to_start_input = Input::default().with_value("3".to_string());
        let actions = popup
            .handle_key_events_internal(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()))
            .expect("boundary case handled");
        assert!(
            !actions.is_empty(),
            "should accept nodes at exact disk boundary"
        );

        // Test one over disk space boundary
        popup.nodes_to_start_input = Input::default().with_value("4".to_string());
        let actions = popup
            .handle_key_events_internal(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()))
            .expect("over boundary handled");
        assert!(actions.is_empty(), "should reject nodes over disk boundary");
    }
}
