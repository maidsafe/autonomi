// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use super::super::utils::centered_rect_fixed;
use super::super::Component;
use crate::{
    action::{Action, OptionsActions},
    mode::{InputMode, Scene},
    style::{clear_area, EUCALYPTUS, GHOST_WHITE, INDIGO, LIGHT_PERIWINKLE, RED, VIVID_SKY_BLUE},
    widgets::hyperlink::Hyperlink,
};
use arboard::Clipboard;
use color_eyre::Result;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::{prelude::*, widgets::*};
use regex::Regex;
use tui_input::{backend::crossterm::EventHandler, Input};

const INPUT_SIZE_REWARDS_ADDRESS: u16 = 42; // Etherum address plus 0x
const INPUT_AREA_REWARDS_ADDRESS: u16 = INPUT_SIZE_REWARDS_ADDRESS + 2; // +2 for the padding

pub struct RewardsAddress {
    /// Whether the component is active right now, capturing keystrokes + draw things.
    active: bool,
    state: RewardsAddressState,
    rewards_address_input_field: Input,
    // cache the old value incase user presses Esc.
    old_value: String,
    back_to: Scene,
    can_save: bool,
}

enum RewardsAddressState {
    RewardsAddressAlreadySet,
    ShowTCs,
    AcceptTCsAndEnterRewardsAddress,
}

impl RewardsAddress {
    pub fn new(rewards_address: String) -> Self {
        let state = if rewards_address.is_empty() {
            RewardsAddressState::ShowTCs
        } else {
            RewardsAddressState::RewardsAddressAlreadySet
        };
        Self {
            active: false,
            state,
            rewards_address_input_field: Input::default().with_value(rewards_address),
            old_value: Default::default(),
            back_to: Scene::Status,
            can_save: false,
        }
    }

    pub fn validate(&mut self) {
        if self.rewards_address_input_field.value().is_empty() {
            self.can_save = false;
        } else {
            let re = Regex::new(r"^0x[a-fA-F0-9]{40}$").expect("Failed to compile regex");
            self.can_save = re.is_match(self.rewards_address_input_field.value());
        }
    }

    fn capture_inputs(&mut self, key: KeyEvent) -> Vec<Action> {
        let send_back = match key.code {
            KeyCode::Enter => {
                self.validate();
                if self.can_save {
                    let rewards_address = self
                        .rewards_address_input_field
                        .value()
                        .to_string()
                        .to_lowercase();
                    self.rewards_address_input_field = rewards_address.clone().into();

                    debug!(
                        "Got Enter, saving the rewards address {rewards_address:?}  and switching to RewardsAddressAlreadySet, and Home Scene",
                    );
                    self.state = RewardsAddressState::RewardsAddressAlreadySet;
                    return vec![
                        Action::StoreRewardsAddress(rewards_address.clone()),
                        Action::OptionsActions(OptionsActions::UpdateRewardsAddress(
                            rewards_address,
                        )),
                        Action::SwitchScene(Scene::Status),
                    ];
                }
                vec![]
            }
            KeyCode::Esc => {
                debug!(
                    "Got Esc, restoring the old value {} and switching to actual screen",
                    self.old_value
                );
                // reset to old value
                self.rewards_address_input_field = self
                    .rewards_address_input_field
                    .clone()
                    .with_value(self.old_value.clone());
                vec![Action::SwitchScene(self.back_to)]
            }
            KeyCode::Char(' ') => vec![],
            KeyCode::Backspace => {
                // if max limit reached, we should allow Backspace to work.
                self.rewards_address_input_field
                    .handle_event(&Event::Key(key));
                self.validate();
                vec![]
            }
            KeyCode::Char('v') => {
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    let mut clipboard = match Clipboard::new() {
                        Ok(clipboard) => clipboard,
                        Err(e) => {
                            error!("Error reading Clipboard : {:?}", e);
                            return vec![];
                        }
                    };
                    if let Ok(content) = clipboard.get_text() {
                        self.rewards_address_input_field =
                            self.rewards_address_input_field.clone().with_value(content);
                    }
                }
                vec![]
            }
            _ => {
                if self.rewards_address_input_field.value().chars().count()
                    < INPUT_SIZE_REWARDS_ADDRESS as usize
                {
                    self.rewards_address_input_field
                        .handle_event(&Event::Key(key));
                    self.validate();
                }
                vec![]
            }
        };
        send_back
    }
}

impl Component for RewardsAddress {
    fn handle_key_events(&mut self, key: KeyEvent) -> Result<Vec<Action>> {
        if !self.active {
            return Ok(vec![]);
        }
        // while in entry mode, keybinds are not captured, so gotta exit entry mode from here
        let send_back = match &self.state {
            RewardsAddressState::RewardsAddressAlreadySet => self.capture_inputs(key),
            RewardsAddressState::ShowTCs => match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    if !self.rewards_address_input_field.value().is_empty() {
                        debug!("User accepted the TCs, but rewards address already set, moving to RewardsAddressAlreadySet");
                        self.state = RewardsAddressState::RewardsAddressAlreadySet;
                    } else {
                        debug!("User accepted the TCs, but no rewards address set, moving to AcceptTCsAndEnterRewardsAddress");
                        self.state = RewardsAddressState::AcceptTCsAndEnterRewardsAddress;
                    }
                    vec![]
                }
                KeyCode::Esc => {
                    debug!("User rejected the TCs, moving to original screen");
                    self.state = RewardsAddressState::ShowTCs;
                    vec![Action::SwitchScene(self.back_to)]
                }
                _ => {
                    vec![]
                }
            },
            RewardsAddressState::AcceptTCsAndEnterRewardsAddress => self.capture_inputs(key),
        };
        Ok(send_back)
    }

    fn update(&mut self, action: Action) -> Result<Option<Action>> {
        let send_back = match action {
            Action::SwitchScene(scene) => match scene {
                Scene::StatusRewardsAddressPopUp | Scene::OptionsRewardsAddressPopUp => {
                    self.active = true;
                    self.old_value = self.rewards_address_input_field.value().to_string();
                    if scene == Scene::StatusRewardsAddressPopUp {
                        self.back_to = Scene::Status;
                    } else if scene == Scene::OptionsRewardsAddressPopUp {
                        self.back_to = Scene::Options;
                    }
                    // Set to InputMode::Entry as we want to handle everything within our handle_key_events
                    // so by default if this scene is active, we capture inputs.
                    Some(Action::SwitchInputMode(InputMode::Entry))
                }
                _ => {
                    self.active = false;
                    None
                }
            },
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
                Constraint::Min(1),
                // for the pop_up_border
                Constraint::Length(1),
            ],
        )
        .split(layer_zero);

        // layer zero
        let pop_up_border = Paragraph::new("").block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Add Your Wallet ")
                .bold()
                .title_style(Style::new().fg(VIVID_SKY_BLUE))
                .padding(Padding::uniform(2))
                .border_style(Style::new().fg(VIVID_SKY_BLUE)),
        );
        clear_area(f, layer_zero);

        match self.state {
            RewardsAddressState::RewardsAddressAlreadySet => {
                self.validate(); // FIXME: maybe this should be somewhere else
                                 // split into 4 parts, for the prompt, input, text, dash , and buttons
                let layer_two = Layout::new(
                    Direction::Vertical,
                    [
                        // for the prompt text
                        Constraint::Length(3),
                        // for the input
                        Constraint::Length(1),
                        // for the text
                        Constraint::Length(6),
                        // gap
                        Constraint::Length(1),
                        // for the buttons
                        Constraint::Length(1),
                    ],
                )
                .split(layer_one[1]);

                let prompt_text = Paragraph::new(Line::from(vec![
                    Span::styled("Enter new ".to_string(), Style::default()),
                    Span::styled("Wallet Address".to_string(), Style::default().bold()),
                ]))
                .block(Block::default())
                .alignment(Alignment::Center)
                .fg(GHOST_WHITE);

                f.render_widget(prompt_text, layer_two[0]);

                let spaces = " ".repeat(
                    (INPUT_AREA_REWARDS_ADDRESS - 1) as usize
                        - self.rewards_address_input_field.value().len(),
                );
                let input = Paragraph::new(Span::styled(
                    format!("{}{} ", spaces, self.rewards_address_input_field.value()),
                    Style::default()
                        .fg(if self.can_save { VIVID_SKY_BLUE } else { RED })
                        .bg(INDIGO)
                        .underlined(),
                ))
                .alignment(Alignment::Center);
                f.render_widget(input, layer_two[1]);

                let text = Paragraph::new(Text::from(if self.can_save {
                    vec![
                        Line::raw("Changing your Wallet will reset and restart"),
                        Line::raw("all your nodes."),
                    ]
                } else {
                    vec![Line::from(Span::styled(
                        "Invalid wallet address".to_string(),
                        Style::default().fg(RED),
                    ))]
                }))
                .alignment(Alignment::Center)
                .block(
                    Block::default()
                        .padding(Padding::horizontal(2))
                        .padding(Padding::top(2)),
                );

                f.render_widget(text.fg(GHOST_WHITE), layer_two[2]);

                let dash = Block::new()
                    .borders(Borders::BOTTOM)
                    .border_style(Style::new().fg(GHOST_WHITE));
                f.render_widget(dash, layer_two[3]);

                let buttons_layer = Layout::horizontal(vec![
                    Constraint::Percentage(55),
                    Constraint::Percentage(45),
                ])
                .split(layer_two[4]);

                let button_no = Line::from(vec![Span::styled(
                    "  Cancel [Esc]",
                    Style::default().fg(LIGHT_PERIWINKLE),
                )]);

                f.render_widget(button_no, buttons_layer[0]);

                let button_yes = Line::from(vec![Span::styled(
                    "Change Wallet [Enter]",
                    if self.can_save {
                        Style::default().fg(EUCALYPTUS)
                    } else {
                        Style::default().fg(LIGHT_PERIWINKLE)
                    },
                )]);
                f.render_widget(button_yes, buttons_layer[1]);
            }
            RewardsAddressState::ShowTCs => {
                // split the area into 3 parts, for the lines, hypertext,  buttons
                let layer_two = Layout::new(
                    Direction::Vertical,
                    [
                        // for the text
                        Constraint::Length(7),
                        // for the hypertext
                        Constraint::Length(1),
                        // gap
                        Constraint::Length(5),
                        // for the buttons
                        Constraint::Length(1),
                    ],
                )
                .split(layer_one[1]);

                let text = Paragraph::new(vec![
                    Line::from(Span::styled("Add your wallet to store your node earnings, and we'll pay you rewards to the same wallet after the Network's Token Generation Event.",Style::default())),
                    Line::from(Span::styled("\n\n",Style::default())),
                    Line::from(Span::styled("By continuing you agree to the Terms and Conditions found here:",Style::default())),
                    Line::from(Span::styled("\n\n",Style::default())),
                    ]
                )
                .block(Block::default().padding(Padding::horizontal(2)))
                .wrap(Wrap { trim: false });

                f.render_widget(text.fg(GHOST_WHITE), layer_two[0]);

                let link = Hyperlink::new(
                    Span::styled(
                        "  https://autonomi.com/beta/terms",
                        Style::default().fg(VIVID_SKY_BLUE),
                    ),
                    "https://autonomi.com/beta/terms",
                );

                f.render_widget_ref(link, layer_two[1]);

                let dash = Block::new()
                    .borders(Borders::BOTTOM)
                    .border_style(Style::new().fg(GHOST_WHITE));
                f.render_widget(dash, layer_two[2]);

                let buttons_layer = Layout::horizontal(vec![
                    Constraint::Percentage(45),
                    Constraint::Percentage(55),
                ])
                .split(layer_two[3]);

                let button_no = Line::from(vec![Span::styled(
                    "  No, Cancel [Esc]",
                    Style::default().fg(LIGHT_PERIWINKLE),
                )]);
                f.render_widget(button_no, buttons_layer[0]);

                let button_yes = Paragraph::new(Line::from(vec![Span::styled(
                    "Yes, I agree! Continue [Y]  ",
                    Style::default().fg(EUCALYPTUS),
                )]))
                .alignment(Alignment::Right);
                f.render_widget(button_yes, buttons_layer[1]);
            }
            RewardsAddressState::AcceptTCsAndEnterRewardsAddress => {
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
                        // for the hyperlink
                        Constraint::Length(2),
                        // gap
                        Constraint::Length(1),
                        // for the buttons
                        Constraint::Length(1),
                    ],
                )
                .split(layer_one[1]);

                let prompt = Paragraph::new(Line::from(vec![
                    Span::styled("Enter your ", Style::default()),
                    Span::styled("Wallet Address", Style::default().fg(GHOST_WHITE)),
                ]))
                .alignment(Alignment::Center);

                f.render_widget(prompt.fg(GHOST_WHITE), layer_two[0]);

                let spaces = " ".repeat(
                    (INPUT_AREA_REWARDS_ADDRESS - 1) as usize
                        - self.rewards_address_input_field.value().len(),
                );
                let input = Paragraph::new(Span::styled(
                    format!("{}{} ", spaces, self.rewards_address_input_field.value()),
                    Style::default().fg(VIVID_SKY_BLUE).bg(INDIGO).underlined(),
                ))
                .alignment(Alignment::Center);
                f.render_widget(input, layer_two[1]);

                let text = Paragraph::new(vec![Line::from(Span::styled(
                    "Find out more about compatible wallets, and how to track your earnings:",
                    Style::default(),
                ))])
                .block(Block::default().padding(Padding::horizontal(2)))
                .wrap(Wrap { trim: false });

                f.render_widget(text.fg(GHOST_WHITE), layer_two[2]);

                let link = Hyperlink::new(
                    Span::styled(
                        "  https://autonomi.com/wallet",
                        Style::default().fg(VIVID_SKY_BLUE),
                    ),
                    "https://autonomi.com/wallet",
                );

                f.render_widget_ref(link, layer_two[3]);

                let dash = Block::new()
                    .borders(Borders::BOTTOM)
                    .border_style(Style::new().fg(GHOST_WHITE));
                f.render_widget(dash, layer_two[4]);

                let buttons_layer = Layout::horizontal(vec![
                    Constraint::Percentage(50),
                    Constraint::Percentage(50),
                ])
                .split(layer_two[5]);

                let button_no = Line::from(vec![Span::styled(
                    "  Cancel [Esc]",
                    Style::default().fg(LIGHT_PERIWINKLE),
                )]);
                f.render_widget(button_no, buttons_layer[0]);
                let button_yes = Paragraph::new(Line::from(vec![Span::styled(
                    "Save Wallet [Enter]  ",
                    if self.can_save {
                        Style::default().fg(EUCALYPTUS)
                    } else {
                        Style::default().fg(LIGHT_PERIWINKLE)
                    },
                )]))
                .alignment(Alignment::Right);
                f.render_widget(button_yes, buttons_layer[1]);
            }
        }

        f.render_widget(pop_up_border, layer_zero);

        Ok(())
    }
}
