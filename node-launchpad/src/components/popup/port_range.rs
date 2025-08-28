// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use std::rc::Rc;

use super::super::super::node_mgmt::{PORT_MAX, PORT_MIN};
use super::super::Component;
use super::super::utils::centered_rect_fixed;
use super::manage_nodes::MAX_NODE_COUNT;
use crate::style::RED;
use crate::{
    action::{Action, OptionsActions},
    connection_mode::ConnectionMode,
    focus::{EventResult, FocusManager, FocusTarget},
    mode::{InputMode, Scene},
    style::{EUCALYPTUS, GHOST_WHITE, INDIGO, LIGHT_PERIWINKLE, VIVID_SKY_BLUE, clear_area},
};
use color_eyre::Result;
use crossterm::event::{Event, KeyCode, KeyEvent};
use ratatui::{prelude::*, widgets::*};
use tui_input::{Input, backend::crossterm::EventHandler};

pub const PORT_ALLOCATION: u32 = MAX_NODE_COUNT as u32 - 1; // We count the port_from as well
const INPUT_SIZE: u32 = 5;
const INPUT_AREA: u32 = INPUT_SIZE + 2; // +2 for the left and right padding

#[derive(Default)]
enum PortRangeState {
    #[default]
    Selection,
    ConfirmChange,
    PortForwardingInfo,
}

pub struct PortRangePopUp {
    state: PortRangeState,
    connection_mode: ConnectionMode,
    connection_mode_old_value: Option<ConnectionMode>,
    port_from: Input,
    port_to: Input,
    port_from_old_value: u32,
    port_to_old_value: u32,
    can_save: bool,
    first_stroke: bool,
}

impl PortRangePopUp {
    pub fn new(connection_mode: ConnectionMode, port_from: u32, port_to: u32) -> Self {
        Self {
            state: PortRangeState::Selection,
            connection_mode,
            connection_mode_old_value: None,
            port_from: Input::default().with_value(port_from.to_string()),
            port_to: Input::default().with_value(port_to.to_string()),
            port_from_old_value: Default::default(),
            port_to_old_value: Default::default(),
            can_save: false,
            first_stroke: true,
        }
    }

    pub fn validate(&mut self) {
        if self.port_from.value().is_empty() {
            self.can_save = false;
        } else {
            let port_from: u32 = self.port_from.value().parse().unwrap_or_default();
            let port_to: u32 = self.port_to.value().parse().unwrap_or_default();
            self.can_save = (PORT_MIN..=PORT_MAX).contains(&port_from)
                && (PORT_MIN..=PORT_MAX).contains(&port_to)
                && port_from <= port_to;
        }
    }

    // -- Draw functions --

    // Draws the Port Selection screen
    fn draw_selection_state(
        &mut self,
        f: &mut crate::tui::Frame<'_>,
        layer_zero: Rect,
        layer_one: Rc<[Rect]>,
    ) -> Paragraph<'_> {
        // layer zero
        let pop_up_border = Paragraph::new("").block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Custom Ports ")
                .bold()
                .title_style(Style::new().fg(VIVID_SKY_BLUE))
                .padding(Padding::uniform(2))
                .border_style(Style::new().fg(VIVID_SKY_BLUE)),
        );
        clear_area(f, layer_zero);

        // split into 4 parts, for the prompt, input, text, dash , and buttons
        let layer_two = Layout::new(
            Direction::Vertical,
            [
                // for the prompt text
                Constraint::Length(3),
                // for the input
                Constraint::Length(2),
                // for the text
                Constraint::Length(3),
                // gap
                Constraint::Length(3),
                // for the buttons
                Constraint::Length(1),
            ],
        )
        .split(layer_one[1]);

        let prompt = Paragraph::new("Enter Port Number")
            .bold()
            .alignment(Alignment::Center);

        f.render_widget(prompt.fg(GHOST_WHITE), layer_two[0]);

        let spaces_from = " ".repeat((INPUT_AREA - 1) as usize - self.port_from.value().len());

        let input_line = Line::from(vec![
            Span::styled(
                format!("{}{} ", spaces_from, self.port_from.value()),
                Style::default()
                    .fg(if self.can_save { VIVID_SKY_BLUE } else { RED })
                    .bg(INDIGO)
                    .underlined(),
            ),
            Span::styled(" to ", Style::default().fg(GHOST_WHITE)),
            Span::styled(self.port_to.value(), Style::default().fg(LIGHT_PERIWINKLE)),
        ])
        .alignment(Alignment::Center);

        f.render_widget(input_line, layer_two[1]);

        let text = Paragraph::new(vec![
            Line::from(Span::styled(
                format!(
                    "Choose the start of the range of {} ports.",
                    PORT_ALLOCATION + 1
                ),
                Style::default().fg(LIGHT_PERIWINKLE),
            )),
            Line::from(Span::styled(
                format!("This must be between {PORT_MIN} and {PORT_MAX}."),
                Style::default().fg(if self.can_save { LIGHT_PERIWINKLE } else { RED }),
            )),
        ])
        .block(block::Block::default().padding(Padding::horizontal(2)))
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: true });
        f.render_widget(text.fg(GHOST_WHITE), layer_two[2]);

        let dash = Block::new()
            .borders(Borders::BOTTOM)
            .border_style(Style::new().fg(GHOST_WHITE));
        f.render_widget(dash, layer_two[3]);

        let buttons_layer =
            Layout::horizontal(vec![Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(layer_two[4]);

        let button_no = Line::from(vec![Span::styled(
            "  Cancel [Esc]",
            Style::default().fg(LIGHT_PERIWINKLE),
        )]);
        let button_yes_style = if self.can_save {
            Style::default().fg(EUCALYPTUS)
        } else {
            Style::default().fg(LIGHT_PERIWINKLE)
        };
        f.render_widget(button_no, buttons_layer[0]);

        let button_yes = Line::from(vec![
            Span::styled("Save Port Range ", button_yes_style),
            Span::styled("[Enter]", Style::default().fg(GHOST_WHITE)),
        ]);
        f.render_widget(button_yes, buttons_layer[1]);

        pop_up_border
    }

    // Draws Confirmation screen
    fn draw_confirm_and_reset(
        &mut self,
        f: &mut crate::tui::Frame<'_>,
        layer_zero: Rect,
        layer_one: Rc<[Rect]>,
    ) -> Paragraph<'_> {
        // layer zero
        let pop_up_border = Paragraph::new("").block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Confirm & Reset ")
                .bold()
                .title_style(Style::new().fg(VIVID_SKY_BLUE))
                .padding(Padding::uniform(2))
                .border_style(Style::new().fg(VIVID_SKY_BLUE)),
        );
        clear_area(f, layer_zero);

        // split into 3 parts, paragraph, dash, buttons
        let layer_two = Layout::new(
            Direction::Vertical,
            [
                // for the text
                Constraint::Length(8),
                // gap
                Constraint::Length(3),
                // for the buttons
                Constraint::Length(1),
            ],
        )
        .split(layer_one[1]);

        let paragraph_text = Paragraph::new(vec![
            Line::from(Span::styled("\n\n", Style::default())),
            Line::from(Span::styled("\n\n", Style::default())),
            Line::from(vec![
                Span::styled(
                    "Changing connection mode will ",
                    Style::default().fg(LIGHT_PERIWINKLE),
                ),
                Span::styled("reset all nodes.", Style::default().fg(GHOST_WHITE)),
            ]),
            Line::from(Span::styled("\n\n", Style::default())),
            Line::from(Span::styled("\n\n", Style::default())),
            Line::from(Span::styled("\n\n", Style::default())),
            Line::from(vec![
                Span::styled("You’ll need to ", Style::default().fg(LIGHT_PERIWINKLE)),
                Span::styled("Add", Style::default().fg(GHOST_WHITE)),
                Span::styled(" and ", Style::default().fg(LIGHT_PERIWINKLE)),
                Span::styled("Start", Style::default().fg(GHOST_WHITE)),
                Span::styled(
                    " them again afterwards. Are you sure you want to continue?",
                    Style::default().fg(LIGHT_PERIWINKLE),
                ),
            ]),
        ])
        .alignment(Alignment::Left)
        .wrap(Wrap { trim: true })
        .block(block::Block::default().padding(Padding::horizontal(2)));

        f.render_widget(paragraph_text, layer_two[0]);

        let dash = Block::new()
            .borders(Borders::BOTTOM)
            .border_style(Style::new().fg(GHOST_WHITE));
        f.render_widget(dash, layer_two[1]);

        let buttons_layer =
            Layout::horizontal(vec![Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(layer_two[2]);

        let button_no = Line::from(vec![Span::styled(
            "  Cancel [Esc]",
            Style::default().fg(LIGHT_PERIWINKLE),
        )]);
        let button_yes_style = if self.can_save {
            Style::default().fg(EUCALYPTUS)
        } else {
            Style::default().fg(LIGHT_PERIWINKLE)
        };
        f.render_widget(button_no, buttons_layer[0]);

        let button_yes = Line::from(vec![
            Span::styled("Yes, Change Mode ", button_yes_style),
            Span::styled("[Enter]", Style::default().fg(GHOST_WHITE)),
        ]);
        f.render_widget(button_yes, buttons_layer[1]);

        pop_up_border
    }

    // Draws info regarding router and ports
    fn draw_info_port_forwarding(
        &mut self,
        f: &mut crate::tui::Frame<'_>,
        layer_zero: Rect,
        layer_one: Rc<[Rect]>,
    ) -> Paragraph<'_> {
        // layer zero
        let pop_up_border = Paragraph::new("").block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Port Forwarding For Private IPs ")
                .bold()
                .title_style(Style::new().fg(VIVID_SKY_BLUE))
                .padding(Padding::uniform(2))
                .border_style(Style::new().fg(VIVID_SKY_BLUE)),
        );
        clear_area(f, layer_zero);

        // split into 3 parts, 1 paragraph, dash and buttons
        let layer_two = Layout::new(
            Direction::Vertical,
            [
                // for the text
                Constraint::Length(8),
                // gap
                Constraint::Length(3),
                // for the buttons
                Constraint::Length(1),
            ],
        )
        .split(layer_one[1]);

        let paragraph_text = Paragraph::new(vec![
            Line::from(Span::styled("\n\n",Style::default())),
            Line::from(Span::styled("If you have a Private IP (which you probably do) you’ll now need to set your router to…\n\n", Style::default().fg(LIGHT_PERIWINKLE))),
            Line::from(Span::styled("\n\n",Style::default())),
            Line::from(Span::styled(
                format!("Port Forward ports {}-{} ", self.port_from.value(), self.port_to.value()),
                Style::default().fg(GHOST_WHITE),
            )),
            Line::from(Span::styled("\n\n",Style::default())),
            Line::from(Span::styled("You can do this in your router’s admin panel.\n\n", Style::default().fg(LIGHT_PERIWINKLE))),
        ])
        .alignment(Alignment::Left)
        .wrap(Wrap { trim: true })
        .block(block::Block::default().padding(Padding::horizontal(2)));

        f.render_widget(paragraph_text, layer_two[0]);

        let dash = Block::new()
            .borders(Borders::BOTTOM)
            .border_style(Style::new().fg(GHOST_WHITE));
        f.render_widget(dash, layer_two[1]);

        let buttons_layer =
            Layout::horizontal(vec![Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(layer_two[2]);

        let button_ok = Line::from(vec![
            Span::styled("OK ", Style::default().fg(EUCALYPTUS)),
            Span::styled("[Enter]   ", Style::default().fg(GHOST_WHITE)),
        ])
        .alignment(Alignment::Right);

        f.render_widget(button_ok, buttons_layer[1]);

        pop_up_border
    }
}

impl Component for PortRangePopUp {
    fn handle_key_events(
        &mut self,
        key: KeyEvent,
        focus_manager: &FocusManager,
    ) -> Result<(Vec<Action>, EventResult)> {
        if !focus_manager.has_focus(&FocusTarget::PortRangePopup) {
            return Ok((vec![], EventResult::Ignored));
        }
        // while in entry mode, keybinds are not captured, so gotta exit entry mode from here
        let send_back: Vec<Action> = match &self.state {
            PortRangeState::Selection => {
                match key.code {
                    KeyCode::Enter => {
                        if self.port_from_old_value
                            == self.port_from.value().parse::<u32>().unwrap_or_default()
                            && self.port_to_old_value
                                == self.port_to.value().parse::<u32>().unwrap_or_default()
                            && self.connection_mode_old_value != Some(ConnectionMode::CustomPorts)
                            && self.can_save
                        {
                            self.state = PortRangeState::ConfirmChange;
                            return Ok((vec![], EventResult::Ignored));
                        }
                        let port_from = self.port_from.value();
                        let port_to = self.port_to.value();

                        if port_from.is_empty() || port_to.is_empty() || !self.can_save {
                            debug!("Got Enter, but port_from or port_to is empty, ignoring.");
                            return Ok((vec![], EventResult::Ignored));
                        }
                        debug!("Got Enter, saving the ports and switching to Options Screen",);
                        self.state = PortRangeState::ConfirmChange;
                        vec![]
                    }
                    KeyCode::Esc => {
                        debug!("Got Esc, restoring the old values and switching to actual screen");
                        if let Some(connection_mode_old_value) = self.connection_mode_old_value {
                            debug!("{:?}", connection_mode_old_value);
                            vec![
                                Action::OptionsActions(OptionsActions::UpdateConnectionMode(
                                    connection_mode_old_value,
                                )),
                                Action::SwitchScene(Scene::Options),
                            ]
                        } else {
                            // if the old values are 0 means that is the first time the user opens the app,
                            // so we should set the connection mode to automatic.
                            if self.port_from_old_value.to_string() == "0"
                                && self.port_to_old_value.to_string() == "0"
                            {
                                self.connection_mode = self
                                    .connection_mode_old_value
                                    .unwrap_or(ConnectionMode::Automatic);
                                let actions = vec![
                                    Action::StoreConnectionMode(self.connection_mode),
                                    Action::OptionsActions(OptionsActions::UpdateConnectionMode(
                                        self.connection_mode,
                                    )),
                                    Action::SwitchScene(Scene::Options),
                                ];
                                return Ok((actions, EventResult::Consumed));
                            }
                            self.port_from = self
                                .port_from
                                .clone()
                                .with_value(self.port_from_old_value.to_string());
                            self.port_to = self
                                .port_to
                                .clone()
                                .with_value(self.port_to_old_value.to_string());
                            vec![Action::SwitchScene(Scene::Options)]
                        }
                    }
                    KeyCode::Char(c) if !c.is_numeric() => vec![],
                    KeyCode::Up => {
                        if self.port_from.value().parse::<u32>().unwrap_or_default() < PORT_MAX {
                            self.port_from = self.port_from.clone().with_value(
                                (self.port_from.value().parse::<u32>().unwrap_or_default() + 1)
                                    .to_string(),
                            );
                            let port_from_value: u32 =
                                self.port_from.value().parse().unwrap_or_default();
                            if port_from_value + PORT_ALLOCATION <= PORT_MAX {
                                self.port_to = Input::default()
                                    .with_value((port_from_value + PORT_ALLOCATION).to_string());
                            } else {
                                self.port_to = Input::default().with_value("-".to_string());
                            }
                        };
                        self.validate();
                        vec![]
                    }
                    KeyCode::Down => {
                        if self.port_from.value().parse::<u32>().unwrap_or_default() > 0 {
                            self.port_from = self.port_from.clone().with_value(
                                (self.port_from.value().parse::<u32>().unwrap_or_default() - 1)
                                    .to_string(),
                            );
                            let port_from_value: u32 =
                                self.port_from.value().parse().unwrap_or_default();
                            if port_from_value + PORT_ALLOCATION <= PORT_MAX {
                                self.port_to = Input::default()
                                    .with_value((port_from_value + PORT_ALLOCATION).to_string());
                            } else {
                                self.port_to = Input::default().with_value("-".to_string());
                            }
                        };
                        self.validate();
                        vec![]
                    }
                    KeyCode::Backspace => {
                        self.port_from.handle_event(&Event::Key(key));
                        let port_from_value: u32 =
                            self.port_from.value().parse().unwrap_or_default();
                        self.port_to = Input::default()
                            .with_value((port_from_value + PORT_ALLOCATION).to_string());
                        self.validate();
                        vec![]
                    }
                    _ => {
                        if self.first_stroke {
                            self.first_stroke = false;
                            self.port_from = Input::default().with_value("".to_string());
                        }
                        // if max limit reached, we should not allow any more inputs.
                        if self.port_from.value().len() < INPUT_SIZE as usize {
                            self.port_from.handle_event(&Event::Key(key));
                            let port_from_value: u32 =
                                self.port_from.value().parse().unwrap_or_default();
                            if port_from_value + PORT_ALLOCATION <= PORT_MAX {
                                self.port_to = Input::default()
                                    .with_value((port_from_value + PORT_ALLOCATION).to_string());
                            } else {
                                self.port_to = Input::default().with_value("-".to_string());
                            }
                        };
                        self.validate();
                        vec![]
                    }
                }
            }
            PortRangeState::ConfirmChange => match key.code {
                KeyCode::Enter => {
                    self.state = PortRangeState::PortForwardingInfo;
                    vec![
                        Action::StoreConnectionMode(ConnectionMode::CustomPorts),
                        Action::OptionsActions(OptionsActions::UpdateConnectionMode(
                            ConnectionMode::CustomPorts,
                        )),
                        Action::StorePortRange(
                            self.port_from.value().parse().unwrap_or_default(),
                            self.port_to.value().parse().unwrap_or_default(),
                        ),
                        Action::OptionsActions(OptionsActions::UpdatePortRange(
                            self.port_from.value().parse().unwrap_or_default(),
                            self.port_to.value().parse().unwrap_or_default(),
                        )),
                    ]
                }
                KeyCode::Esc => {
                    self.state = PortRangeState::Selection;
                    if let Some(connection_mode_old_value) = self.connection_mode_old_value {
                        if self.port_from_old_value != 0 && self.port_to_old_value != 0 {
                            vec![
                                Action::OptionsActions(OptionsActions::UpdateConnectionMode(
                                    connection_mode_old_value,
                                )),
                                Action::OptionsActions(OptionsActions::UpdatePortRange(
                                    self.port_from_old_value,
                                    self.port_to_old_value,
                                )),
                                Action::SwitchScene(Scene::Options),
                            ]
                        } else {
                            vec![
                                Action::OptionsActions(OptionsActions::UpdateConnectionMode(
                                    connection_mode_old_value,
                                )),
                                Action::SwitchScene(Scene::Options),
                            ]
                        }
                    } else {
                        vec![Action::SwitchScene(Scene::Options)]
                    }
                }
                _ => vec![],
            },
            PortRangeState::PortForwardingInfo => match key.code {
                KeyCode::Enter => {
                    debug!("Got Enter, saving the ports and switching to Status Screen",);
                    self.state = PortRangeState::Selection;
                    vec![Action::SwitchScene(Scene::Status)]
                }
                _ => vec![],
            },
        };
        let result = if send_back.is_empty() {
            EventResult::Ignored
        } else {
            EventResult::Consumed
        };
        Ok((send_back, result))
    }

    fn focus_target(&self) -> FocusTarget {
        FocusTarget::PortRangePopup
    }

    fn update(&mut self, action: Action) -> Result<Option<Action>> {
        let send_back = match action {
            Action::SwitchScene(Scene::ChangePortsPopUp {
                connection_mode_old_value,
            }) => {
                if self.connection_mode == ConnectionMode::CustomPorts {
                    self.first_stroke = true;
                    self.connection_mode_old_value = connection_mode_old_value;
                    self.validate();
                    self.port_from_old_value = self.port_from.value().parse().unwrap_or_default();
                    self.port_to_old_value = self.port_to.value().parse().unwrap_or_default();
                    // Set to InputMode::Entry as we want to handle everything within our handle_key_events
                    // so by default if this scene is active, we capture inputs.
                    Some(Action::SwitchInputMode(InputMode::Entry))
                } else {
                    Some(Action::SwitchScene(Scene::Options))
                }
            }
            // Useful when the user has selected a connection mode but didn't confirm it
            Action::OptionsActions(OptionsActions::UpdateConnectionMode(connection_mode)) => {
                self.connection_mode = connection_mode;
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
                Constraint::Min(1),
                // for the pop_up_border
                Constraint::Length(1),
            ],
        )
        .split(layer_zero);

        let pop_up_border: Paragraph = match self.state {
            PortRangeState::Selection => self.draw_selection_state(f, layer_zero, layer_one),
            PortRangeState::ConfirmChange => self.draw_confirm_and_reset(f, layer_zero, layer_one),
            PortRangeState::PortForwardingInfo => {
                self.draw_info_port_forwarding(f, layer_zero, layer_one)
            }
        };
        // We render now so the borders are on top of the other widgets
        f.render_widget(pop_up_border, layer_zero);

        Ok(())
    }
}
