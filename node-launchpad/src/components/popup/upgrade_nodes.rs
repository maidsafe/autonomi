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
    node_mgmt,
    style::{clear_area, EUCALYPTUS, GHOST_WHITE, LIGHT_PERIWINKLE, VIVID_SKY_BLUE},
};
use color_eyre::Result;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{prelude::*, widgets::*};

pub struct UpgradeNodesPopUp {
    /// Whether the component is active right now, capturing keystrokes + draw things.
    active: bool,
}

impl UpgradeNodesPopUp {
    pub fn new() -> Self {
        Self { active: false }
    }
}

impl Default for UpgradeNodesPopUp {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for UpgradeNodesPopUp {
    fn handle_key_events(&mut self, key: KeyEvent) -> Result<Vec<Action>> {
        if !self.active {
            return Ok(vec![]);
        }
        // while in entry mode, keybinds are not captured, so gotta exit entry mode from here
        let send_back = match key.code {
            KeyCode::Enter => {
                debug!("Got Enter, Upgrading nodes...");
                vec![
                    Action::OptionsActions(OptionsActions::UpdateNodes),
                    Action::SwitchScene(Scene::Status),
                ]
            }
            KeyCode::Esc => {
                debug!("Got Esc, Not upgrading nodes.");
                vec![Action::SwitchScene(Scene::Options)]
            }
            _ => vec![],
        };
        Ok(send_back)
    }

    fn update(&mut self, action: Action) -> Result<Option<Action>> {
        let send_back = match action {
            Action::SwitchScene(scene) => match scene {
                Scene::UpgradeNodesPopUp => {
                    self.active = true;
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
                .title(" Upgrade all nodes ")
                .bold()
                .title_style(Style::new().fg(VIVID_SKY_BLUE))
                .padding(Padding::uniform(2))
                .border_style(Style::new().fg(VIVID_SKY_BLUE)),
        );
        clear_area(f, layer_zero);

        // split the area into 3 parts, for the lines, hypertext,  buttons
        let layer_two = Layout::new(
            Direction::Vertical,
            [
                // for the text
                Constraint::Length(10),
                // gap
                Constraint::Length(3),
                // for the buttons
                Constraint::Length(1),
            ],
        )
        .split(layer_one[1]);

        let text = Paragraph::new(vec![
            Line::from(Span::styled("\n\n", Style::default())),
            Line::from(vec![
                Span::styled("This will ", Style::default().fg(LIGHT_PERIWINKLE)),
                Span::styled(
                    "stop and upgrade all nodes. ",
                    Style::default().fg(GHOST_WHITE),
                ),
            ]),
            Line::from(Span::styled(
                "No data will be lost.",
                Style::default().fg(LIGHT_PERIWINKLE),
            )),
            Line::from(Span::styled(
                format!(
                    "Upgrade time is {:.1?} seconds per node",
                    node_mgmt::FIXED_INTERVAL / 1_000,
                ),
                Style::default().fg(LIGHT_PERIWINKLE),
            )),
            Line::from(Span::styled(
                "plus, new binary download time.",
                Style::default().fg(LIGHT_PERIWINKLE),
            )),
            Line::from(Span::styled("\n\n", Style::default())),
            Line::from(vec![
                Span::styled("You’ll need to ", Style::default().fg(LIGHT_PERIWINKLE)),
                Span::styled("Start ", Style::default().fg(GHOST_WHITE)),
                Span::styled(
                    "them again afterwards.",
                    Style::default().fg(LIGHT_PERIWINKLE),
                ),
            ]),
            Line::from(Span::styled(
                "Are you sure you want to continue?",
                Style::default(),
            )),
        ])
        .block(Block::default().padding(Padding::horizontal(2)))
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: true });

        f.render_widget(text, layer_two[0]);

        let dash = Block::new()
            .borders(Borders::BOTTOM)
            .border_style(Style::new().fg(GHOST_WHITE));
        f.render_widget(dash, layer_two[1]);

        let buttons_layer =
            Layout::horizontal(vec![Constraint::Percentage(45), Constraint::Percentage(55)])
                .split(layer_two[2]);

        let button_no = Line::from(vec![Span::styled(
            "  No, Cancel [Esc]",
            Style::default().fg(LIGHT_PERIWINKLE),
        )]);
        f.render_widget(button_no, buttons_layer[0]);

        let button_yes = Paragraph::new(Line::from(vec![Span::styled(
            "Yes, Upgrade [Enter]  ",
            Style::default().fg(EUCALYPTUS),
        )]))
        .alignment(Alignment::Right);
        f.render_widget(button_yes, buttons_layer[1]);
        f.render_widget(pop_up_border, layer_zero);

        Ok(())
    }
}
