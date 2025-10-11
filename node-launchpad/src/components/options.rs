// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use ant_evm::EvmAddress;
use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Row, Table},
};
use std::{any::Any, cmp::max, path::PathBuf};
use tokio::sync::mpsc::UnboundedSender;
use tui_input::{Input, backend::crossterm::EventHandler};

use super::{Component, header::SelectedMenuItem, utils::open_logs};
use crate::{
    action::{Action, OptionsActions},
    components::{header::Header, popup::manage_nodes::MAX_NODE_COUNT},
    focus::{EventResult, FocusManager, FocusTarget},
    mode::{InputMode, Scene},
    node_management::config::{PORT_MAX, PORT_MIN},
    style::{
        COOL_GREY, EUCALYPTUS, GHOST_WHITE, INDIGO, LIGHT_PERIWINKLE, VERY_LIGHT_AZURE,
        VIVID_SKY_BLUE,
    },
};

const PORT_ALLOCATION: u32 = (MAX_NODE_COUNT - 1) as u32;

#[derive(Clone)]
pub struct Options {
    pub storage_mountpoint: PathBuf,
    pub storage_drive: String,
    pub rewards_address: Option<EvmAddress>,
    pub upnp_enabled: bool,
    pub port_range: Option<(u32, u32)>,
    pub port_edit_mode: bool,
    pub port_input: Input,
    pub action_tx: Option<UnboundedSender<Action>>,
}

impl Options {
    pub fn new(
        storage_mountpoint: PathBuf,
        storage_drive: String,
        rewards_address: Option<EvmAddress>,
        upnp_enabled: bool,
        port_range: Option<(u32, u32)>,
    ) -> Self {
        Self {
            storage_mountpoint,
            storage_drive,
            rewards_address,
            upnp_enabled,
            port_range,
            port_edit_mode: false,
            port_input: Input::default(),
            action_tx: None,
        }
    }

    fn validate_port_input(&self) -> Option<u32> {
        if let Ok(port) = self.port_input.value().parse::<u32>() {
            if (PORT_MIN..=PORT_MAX).contains(&port) && port + PORT_ALLOCATION <= PORT_MAX {
                Some(port)
            } else {
                None
            }
        } else {
            None
        }
    }

    fn calculate_port_range(&self, start_port: u32) -> (u32, u32) {
        (start_port, start_port + PORT_ALLOCATION)
    }
}

impl Component for Options {
    fn focus_target(&self) -> crate::focus::FocusTarget {
        crate::focus::FocusTarget::Options
    }

    fn draw(&mut self, f: &mut Frame<'_>, area: Rect) -> Result<()> {
        // Define the layout to split the area into four sections
        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints(
                [
                    Constraint::Length(1),
                    Constraint::Length(5),
                    Constraint::Length(3),
                    Constraint::Length(3),
                    Constraint::Length(4),
                    Constraint::Length(3),
                ]
                .as_ref(),
            )
            .split(area);

        // ==== Header =====
        let header = Header::new();
        f.render_stateful_widget(header, layout[0], &mut SelectedMenuItem::Options);

        // Storage Drive
        let block1 = Block::default()
            .title(" Device Options ")
            .title_style(Style::default().bold().fg(GHOST_WHITE))
            .style(Style::default().fg(GHOST_WHITE))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(VERY_LIGHT_AZURE));
        let storage_drivename = Table::new(
            vec![
                Row::new(vec![
                    Cell::from(
                        Line::from(vec![Span::styled(
                            " Storage Drive: ",
                            Style::default().fg(LIGHT_PERIWINKLE),
                        )])
                        .alignment(Alignment::Left),
                    ),
                    Cell::from(
                        Line::from(vec![Span::styled(
                            format!(" {} ", self.storage_drive),
                            Style::default().fg(VIVID_SKY_BLUE),
                        )])
                        .alignment(Alignment::Left),
                    ),
                    Cell::from(
                        Line::from(vec![
                            Span::styled(" Change Drive ", Style::default().fg(VERY_LIGHT_AZURE)),
                            Span::styled(" [Ctrl+D] ", Style::default().fg(GHOST_WHITE)),
                        ])
                        .alignment(Alignment::Right),
                    ),
                ]),
                Row::new(vec![
                    Cell::from(
                        Line::from(vec![Span::styled(
                            " UPnP: ",
                            Style::default().fg(LIGHT_PERIWINKLE),
                        )])
                        .alignment(Alignment::Left),
                    ),
                    Cell::from(
                        Line::from(if self.upnp_enabled {
                            vec![Span::styled(" Enabled ", Style::default().fg(EUCALYPTUS))]
                        } else {
                            vec![
                                Span::styled(" Disabled ", Style::default().fg(COOL_GREY)),
                                Span::styled(
                                    "(recommend enabling)",
                                    Style::default().fg(LIGHT_PERIWINKLE),
                                ),
                            ]
                        })
                        .alignment(Alignment::Left),
                    ),
                    Cell::from(
                        Line::from(vec![
                            Span::styled(" Toggle UPnP ", Style::default().fg(VERY_LIGHT_AZURE)),
                            Span::styled(" [Ctrl+U] ", Style::default().fg(GHOST_WHITE)),
                        ])
                        .alignment(Alignment::Right),
                    ),
                ]),
                Row::new(vec![
                    Cell::from(
                        Line::from(vec![Span::styled(
                            " Port Range: ",
                            Style::default().fg(LIGHT_PERIWINKLE),
                        )])
                        .alignment(Alignment::Left),
                    ),
                    Cell::from(
                        if self.port_edit_mode {
                            Line::from(vec![
                                Span::styled(" > ", Style::default().fg(VIVID_SKY_BLUE)),
                                Span::styled(
                                    self.port_input.value(),
                                    Style::default()
                                        .fg(if self.validate_port_input().is_some() {
                                            VIVID_SKY_BLUE
                                        } else {
                                            COOL_GREY
                                        })
                                        .bg(INDIGO),
                                ),
                                Span::styled(" [Enter/Esc]", Style::default().fg(LIGHT_PERIWINKLE)),
                            ])
                        } else {
                            Line::from(vec![if let Some((from, to)) = self.port_range {
                                Span::styled(
                                    format!(" {from}-{to} "),
                                    Style::default().fg(VIVID_SKY_BLUE),
                                )
                            } else {
                                Span::styled(" Auto ", Style::default().fg(COOL_GREY))
                            }])
                        }
                        .alignment(Alignment::Left),
                    ),
                    Cell::from(
                        Line::from(vec![
                            Span::styled(" Edit Range ", Style::default().fg(VERY_LIGHT_AZURE)),
                            Span::styled(" [Ctrl+P] ", Style::default().fg(GHOST_WHITE)),
                        ])
                        .alignment(Alignment::Right),
                    ),
                ]),
            ],
            &[
                Constraint::Length(18),
                Constraint::Fill(1),
                Constraint::Length(25),
            ],
        )
        .block(block1)
        .style(Style::default().fg(GHOST_WHITE));

        // Beta Rewards Program
        let beta_legend = if self.rewards_address.is_none() {
            " Add Wallet "
        } else {
            " Change Wallet "
        };
        let beta_key = " [Ctrl+B] ";
        let block2 = Block::default()
            .title(" Wallet ")
            .title_style(Style::default().bold().fg(GHOST_WHITE))
            .style(Style::default().fg(GHOST_WHITE))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(VERY_LIGHT_AZURE));
        let rewards_address_str = match self.rewards_address {
            Some(ref addr) => addr.to_string(),
            None => "".to_string(),
        };
        let beta_rewards = Table::new(
            vec![Row::new(vec![
                Cell::from(
                    Line::from(vec![Span::styled(
                        " Wallet Address: ",
                        Style::default().fg(LIGHT_PERIWINKLE),
                    )])
                    .alignment(Alignment::Left),
                ),
                Cell::from(
                    Line::from(vec![Span::styled(
                        format!(" {rewards_address_str} "),
                        Style::default().fg(VIVID_SKY_BLUE),
                    )])
                    .alignment(Alignment::Left),
                ),
                Cell::from(
                    Line::from(vec![
                        Span::styled(beta_legend, Style::default().fg(VERY_LIGHT_AZURE)),
                        Span::styled(beta_key, Style::default().fg(GHOST_WHITE)),
                    ])
                    .alignment(Alignment::Right),
                ),
            ])],
            &[
                Constraint::Length(18),
                Constraint::Fill(1),
                Constraint::Length((beta_legend.len() + beta_key.len()) as u16),
            ],
        )
        .block(block2)
        .style(Style::default().fg(GHOST_WHITE));

        // Access Logs
        let logs_legend = " Access Logs ";
        let logs_key = " [Ctrl+L] ";
        let block3 = Block::default()
            .title(" Access Logs ")
            .title_style(Style::default().bold().fg(GHOST_WHITE))
            .style(Style::default().fg(GHOST_WHITE))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(VERY_LIGHT_AZURE));
        let logs_folder = Table::new(
            vec![Row::new(vec![
                Cell::from(
                    Line::from(vec![Span::styled(
                        " Open the logs folder on this device ",
                        Style::default().fg(LIGHT_PERIWINKLE),
                    )])
                    .alignment(Alignment::Left),
                ),
                Cell::from(
                    Line::from(vec![
                        Span::styled(logs_legend, Style::default().fg(VERY_LIGHT_AZURE)),
                        Span::styled(logs_key, Style::default().fg(GHOST_WHITE)),
                    ])
                    .alignment(Alignment::Right),
                ),
            ])],
            &[
                Constraint::Fill(1),
                Constraint::Length((logs_legend.len() + logs_key.len()) as u16),
            ],
        )
        .block(block3)
        .style(Style::default().fg(GHOST_WHITE));

        // Update Nodes
        let reset_legend = " Begin Reset ";
        let reset_key = " [Ctrl+R] ";
        let upgrade_legend = " Begin Upgrade ";
        let upgrade_key = " [Ctrl+G] ";
        let block4 = Block::default()
            .title(" Update Nodes ")
            .title_style(Style::default().bold().fg(GHOST_WHITE))
            .style(Style::default().fg(GHOST_WHITE))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(EUCALYPTUS));
        let reset_nodes = Table::new(
            vec![
                Row::new(vec![
                    Cell::from(
                        Line::from(vec![Span::styled(
                            " Upgrade all nodes ",
                            Style::default().fg(LIGHT_PERIWINKLE),
                        )])
                        .alignment(Alignment::Left),
                    ),
                    Cell::from(
                        Line::from(vec![
                            Span::styled(upgrade_legend, Style::default().fg(EUCALYPTUS)),
                            Span::styled(upgrade_key, Style::default().fg(GHOST_WHITE)),
                        ])
                        .alignment(Alignment::Right),
                    ),
                ]),
                Row::new(vec![
                    Cell::from(
                        Line::from(vec![Span::styled(
                            " Reset all nodes on this device ",
                            Style::default().fg(LIGHT_PERIWINKLE),
                        )])
                        .alignment(Alignment::Left),
                    ),
                    Cell::from(
                        Line::from(vec![
                            Span::styled(reset_legend, Style::default().fg(EUCALYPTUS)),
                            Span::styled(reset_key, Style::default().fg(GHOST_WHITE)),
                        ])
                        .alignment(Alignment::Right),
                    ),
                ]),
            ],
            &[
                Constraint::Fill(1),
                Constraint::Length(
                    (max(reset_legend.len(), upgrade_legend.len())
                        + max(reset_key.len(), upgrade_key.len())) as u16,
                ),
            ],
        )
        .block(block4)
        .style(Style::default().fg(GHOST_WHITE));

        // Quit
        let quit_legend = "Quit ";
        let quit_key = "[Q] ";
        let block5 = Block::default()
            .style(Style::default().fg(GHOST_WHITE))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(VIVID_SKY_BLUE));
        let quit = Table::new(
            vec![Row::new(vec![
                Cell::from(
                    Line::from(vec![Span::styled(
                        " Close Launchpad (your nodes will keep running in the background) ",
                        Style::default().fg(LIGHT_PERIWINKLE),
                    )])
                    .alignment(Alignment::Left),
                ),
                Cell::from(
                    Line::from(vec![
                        Span::styled(quit_legend, Style::default().fg(VIVID_SKY_BLUE)),
                        Span::styled(quit_key, Style::default().fg(GHOST_WHITE)),
                    ])
                    .alignment(Alignment::Right),
                ),
            ])],
            &[
                Constraint::Fill(1),
                Constraint::Length((quit_legend.len() + quit_key.len()) as u16),
            ],
        )
        .block(block5)
        .style(Style::default().fg(GHOST_WHITE));

        // Render the tables in their respective sections
        f.render_widget(storage_drivename, layout[1]);
        f.render_widget(beta_rewards, layout[2]);
        f.render_widget(logs_folder, layout[3]);
        f.render_widget(reset_nodes, layout[4]);
        f.render_widget(quit, layout[5]);

        Ok(())
    }

    fn handle_key_events(
        &mut self,
        key: KeyEvent,
        focus_manager: &FocusManager,
    ) -> Result<(Vec<Action>, EventResult)> {
        if !focus_manager.has_focus(&FocusTarget::Options) || !self.port_edit_mode {
            return Ok((vec![], EventResult::Ignored));
        }

        match key.code {
            KeyCode::Enter => {
                if let Some(start_port) = self.validate_port_input() {
                    let (from, to) = self.calculate_port_range(start_port);
                    let port_range = Some((from, to));

                    self.port_edit_mode = false;
                    self.port_range = port_range;

                    return Ok((
                        vec![
                            Action::StorePortRange(port_range),
                            Action::SwitchInputMode(InputMode::Navigation),
                        ],
                        EventResult::Consumed,
                    ));
                }
                Ok((vec![], EventResult::Consumed))
            }
            KeyCode::Esc => {
                self.port_edit_mode = false;
                // Reset input field to current port range value
                self.port_input = if let Some((from, _)) = self.port_range {
                    Input::default().with_value(from.to_string())
                } else {
                    Input::default()
                };
                Ok((
                    vec![Action::SwitchInputMode(InputMode::Navigation)],
                    EventResult::Consumed,
                ))
            }
            KeyCode::Char(c) if c.is_numeric() => {
                self.port_input
                    .handle_event(&crossterm::event::Event::Key(key));
                Ok((vec![], EventResult::Consumed))
            }
            KeyCode::Backspace => {
                self.port_input
                    .handle_event(&crossterm::event::Event::Key(key));
                Ok((vec![], EventResult::Consumed))
            }
            _ => Ok((vec![], EventResult::Consumed)),
        }
    }

    fn update(&mut self, action: Action) -> Result<Option<Action>> {
        match action {
            Action::SwitchScene(Scene::Options) => {
                return Ok(Some(Action::SwitchInputMode(InputMode::Navigation)));
            }
            Action::StoreRewardsAddress(rewards_address) => {
                self.rewards_address = Some(rewards_address);
            }
            Action::OptionsActions(action) => match action {
                OptionsActions::TriggerChangeDrive => {
                    return Ok(Some(Action::SwitchScene(Scene::ChangeDrivePopUp)));
                }
                OptionsActions::TriggerPortRangeEdit => {
                    self.port_edit_mode = true;
                    self.port_input = if let Some((from, _)) = self.port_range {
                        Input::default().with_value(from.to_string())
                    } else {
                        Input::default()
                    };
                    return Ok(Some(Action::SwitchInputMode(InputMode::Entry)));
                }
                OptionsActions::UpdateStorageDrive(mountpoint, drive) => {
                    self.storage_mountpoint = mountpoint;
                    self.storage_drive = drive;
                }
                OptionsActions::ToggleUpnpSetting => {
                    self.upnp_enabled = !self.upnp_enabled;
                    return Ok(Some(Action::StoreUpnpSetting(self.upnp_enabled)));
                }
                OptionsActions::TriggerRewardsAddress => {
                    return Ok(Some(Action::SwitchScene(Scene::OptionsRewardsAddressPopUp)));
                }
                OptionsActions::TriggerAccessLogs => {
                    open_logs(None)?;
                }
                OptionsActions::TriggerUpdateNodes => {
                    return Ok(Some(Action::SwitchScene(Scene::UpgradeNodesPopUp)));
                }
                OptionsActions::TriggerResetNodes => {
                    return Ok(Some(Action::SwitchScene(Scene::ResetNodesPopUp)));
                }
            },
            _ => {}
        }
        Ok(None)
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
    use crossterm::event::{KeyEvent, KeyModifiers};

    fn build_options() -> Options {
        Options::new(
            PathBuf::from("/tmp"),
            "drive".into(),
            None,
            true,
            Some((PORT_MIN, PORT_MIN + PORT_ALLOCATION)),
        )
    }

    #[tokio::test]
    async fn handle_key_events_applies_valid_port() {
        let mut options = build_options();
        options.port_edit_mode = true;
        options.port_input = Input::default().with_value((PORT_MIN + 5).to_string());
        let focus_manager = FocusManager::new(FocusTarget::Options);

        let (actions, result) = options
            .handle_key_events(
                KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
                &focus_manager,
            )
            .expect("handled");
        assert_eq!(result, EventResult::Consumed);
        assert_eq!(
            options.port_range,
            Some((PORT_MIN + 5, PORT_MIN + 5 + PORT_ALLOCATION))
        );
        assert!(
            actions
                .iter()
                .any(|a| matches!(a, Action::StorePortRange(_)))
        );
    }

    #[tokio::test]
    async fn handle_key_events_escape_resets_input() {
        let mut options = build_options();
        options.port_edit_mode = true;
        options.port_range = Some((21000, 21000 + PORT_ALLOCATION));
        options.port_input = Input::default().with_value("99999".into());
        let focus_manager = FocusManager::new(FocusTarget::Options);

        let (actions, _) = options
            .handle_key_events(
                KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
                &focus_manager,
            )
            .expect("handled");
        assert_eq!(options.port_input.value(), "21000");
        assert!(actions.contains(&Action::SwitchInputMode(InputMode::Navigation)));
    }

    #[tokio::test]
    async fn update_toggle_upnp_returns_store_action() {
        let mut options = build_options();
        options.upnp_enabled = true;
        let action = options
            .update(Action::OptionsActions(OptionsActions::ToggleUpnpSetting))
            .expect("update")
            .expect("action");
        assert!(!options.upnp_enabled);
        assert_eq!(action, Action::StoreUpnpSetting(false));
    }

    #[tokio::test]
    async fn update_trigger_port_range_edit_enters_entry_mode() {
        let mut options = build_options();
        let action = options
            .update(Action::OptionsActions(OptionsActions::TriggerPortRangeEdit))
            .expect("update")
            .expect("action");
        assert!(options.port_edit_mode);
        assert_eq!(action, Action::SwitchInputMode(InputMode::Entry));
        assert_eq!(options.port_input.value(), PORT_MIN.to_string());
    }

    #[test]
    fn port_validation_edge_cases_and_boundaries() {
        let mut options = build_options();

        // Test exact minimum port
        options.port_input = Input::default().with_value(PORT_MIN.to_string());
        assert_eq!(options.validate_port_input(), Some(PORT_MIN));

        // Test one below minimum
        options.port_input = Input::default().with_value((PORT_MIN - 1).to_string());
        assert_eq!(options.validate_port_input(), None);

        // Test exact maximum allowed (considering allocation)
        let max_allowed = PORT_MAX - PORT_ALLOCATION;
        options.port_input = Input::default().with_value(max_allowed.to_string());
        assert_eq!(options.validate_port_input(), Some(max_allowed));

        // Test one over maximum allowed
        options.port_input = Input::default().with_value((max_allowed + 1).to_string());
        assert_eq!(options.validate_port_input(), None);

        // Test at absolute PORT_MAX (should fail due to allocation)
        options.port_input = Input::default().with_value(PORT_MAX.to_string());
        assert_eq!(options.validate_port_input(), None);

        // Test invalid input formats
        options.port_input = Input::default().with_value("abc".to_string());
        assert_eq!(options.validate_port_input(), None);

        options.port_input = Input::default().with_value("".to_string());
        assert_eq!(options.validate_port_input(), None);

        options.port_input = Input::default().with_value("0".to_string());
        assert_eq!(options.validate_port_input(), None);

        // Test very large numbers
        options.port_input = Input::default().with_value("999999".to_string());
        assert_eq!(options.validate_port_input(), None);
    }

    #[test]
    fn port_range_calculation_comprehensive() {
        let options = build_options();

        // Test basic allocation expansion
        let (from, to) = options.calculate_port_range(20000);
        assert_eq!(from, 20000);
        assert_eq!(to, 20000 + PORT_ALLOCATION);

        // Test minimum port range
        let (from, to) = options.calculate_port_range(PORT_MIN);
        assert_eq!(from, PORT_MIN);
        assert_eq!(to, PORT_MIN + PORT_ALLOCATION);
        assert!(
            to <= PORT_MAX,
            "calculated range should not exceed PORT_MAX"
        );

        // Test maximum valid starting port
        let max_start = PORT_MAX - PORT_ALLOCATION;
        let (from, to) = options.calculate_port_range(max_start);
        assert_eq!(from, max_start);
        assert_eq!(to, PORT_MAX);

        // Test port allocation size consistency
        assert_eq!(to - from, PORT_ALLOCATION);
    }
}
