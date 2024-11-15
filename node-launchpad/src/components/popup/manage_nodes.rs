// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use std::path::PathBuf;

use crate::action::OptionsActions;
use crate::system::get_available_space_b;
use color_eyre::Result;
use crossterm::event::{Event, KeyCode, KeyEvent};
use ratatui::{prelude::*, widgets::*};
use tui_input::{backend::crossterm::EventHandler, Input};

use crate::{
    action::Action,
    mode::{InputMode, Scene},
    style::{clear_area, EUCALYPTUS, GHOST_WHITE, LIGHT_PERIWINKLE, VIVID_SKY_BLUE},
};

use super::super::{utils::centered_rect_fixed, Component};

pub const GB_PER_NODE: usize = 35;
pub const MB: usize = 1000 * 1000;
pub const GB: usize = MB * 1000;
pub const MAX_NODE_COUNT: usize = 50;

pub struct ManageNodes {
    /// Whether the component is active right now, capturing keystrokes + drawing things.
    active: bool,
    available_disk_space_gb: usize,
    storage_mountpoint: PathBuf,
    nodes_to_start_input: Input,
    // cache the old value incase user presses Esc.
    old_value: String,
}

impl ManageNodes {
    pub fn new(nodes_to_start: usize, storage_mountpoint: PathBuf) -> Result<Self> {
        let nodes_to_start = std::cmp::min(nodes_to_start, MAX_NODE_COUNT);
        let new = Self {
            active: false,
            available_disk_space_gb: get_available_space_b(&storage_mountpoint)? / GB,
            nodes_to_start_input: Input::default().with_value(nodes_to_start.to_string()),
            old_value: Default::default(),
            storage_mountpoint: storage_mountpoint.clone(),
        };
        Ok(new)
    }

    fn get_nodes_to_start_val(&self) -> usize {
        self.nodes_to_start_input.value().parse().unwrap_or(0)
    }

    // Returns the max number of nodes to start
    // It is the minimum of the available disk space and the max nodes limit
    fn max_nodes_to_start(&self) -> usize {
        std::cmp::min(self.available_disk_space_gb / GB_PER_NODE, MAX_NODE_COUNT)
    }
}

impl Component for ManageNodes {
    fn handle_key_events(&mut self, key: KeyEvent) -> Result<Vec<Action>> {
        if !self.active {
            return Ok(vec![]);
        }

        // while in entry mode, key bindings are not captured, so gotta exit entry mode from here
        let send_back = match key.code {
            KeyCode::Enter => {
                let nodes_to_start_str = self.nodes_to_start_input.value().to_string();
                let nodes_to_start =
                    std::cmp::min(self.get_nodes_to_start_val(), self.max_nodes_to_start());

                // set the new value
                self.nodes_to_start_input = self
                    .nodes_to_start_input
                    .clone()
                    .with_value(nodes_to_start.to_string());

                debug!(
                        "Got Enter, value found to be {nodes_to_start} derived from input: {nodes_to_start_str:?} and switching scene",
                    );
                vec![
                    Action::StoreNodesToStart(nodes_to_start),
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
                let number = c.to_string().parse::<usize>().unwrap_or(0);
                let new_value = format!("{}{}", self.get_nodes_to_start_val(), number)
                    .parse::<usize>()
                    .unwrap_or(0);
                // if it might exceed the available space or if more than max_node_count, then enter the max
                if new_value * GB_PER_NODE > self.available_disk_space_gb
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
                        if current_val == 0 {
                            0
                        } else {
                            current_val - 1
                        }
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

    fn update(&mut self, action: Action) -> Result<Option<Action>> {
        let send_back = match action {
            Action::SwitchScene(scene) => match scene {
                Scene::ManageNodesPopUp { amount_of_nodes } => {
                    self.nodes_to_start_input = self
                        .nodes_to_start_input
                        .clone()
                        .with_value(amount_of_nodes.to_string());
                    self.active = true;
                    self.old_value = self.nodes_to_start_input.value().to_string();
                    // set to entry input mode as we want to handle everything within our handle_key_events
                    // so by default if this scene is active, we capture inputs.
                    Some(Action::SwitchInputMode(InputMode::Entry))
                }
                _ => {
                    self.active = false;
                    None
                }
            },
            Action::OptionsActions(OptionsActions::UpdateStorageDrive(mountpoint, _drive_name)) => {
                self.storage_mountpoint.clone_from(&mountpoint);
                self.available_disk_space_gb = get_available_space_b(&mountpoint)? / GB;
                None
            }
            _ => None,
        };
        Ok(send_back)
    }

    fn draw(&mut self, f: &mut crate::tui::Frame<'_>, area: Rect) -> Result<()> {
        if !self.active {
            return Ok(());
        }

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
}
