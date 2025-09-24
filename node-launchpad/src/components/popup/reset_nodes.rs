// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use super::super::{Component, utils::centered_rect_fixed};
use crate::{
    action::{Action, NodeManagementCommand, NodeTableActions},
    focus::{EventResult, FocusManager, FocusTarget},
    mode::{InputMode, Scene},
    style::{EUCALYPTUS, GHOST_WHITE, INDIGO, LIGHT_PERIWINKLE, VIVID_SKY_BLUE, clear_area},
};
use color_eyre::Result;
use crossterm::event::{Event, KeyCode, KeyEvent};
use ratatui::{prelude::*, widgets::*};
use std::any::Any;
use tui_input::{Input, backend::crossterm::EventHandler};

const INPUT_SIZE: u16 = 5;
const INPUT_AREA: u16 = INPUT_SIZE + 2; // +2 for the left and right padding

#[derive(Default)]
pub struct ResetNodesPopup {
    confirmation_input_field: Input,
    can_reset: bool,
}

impl Component for ResetNodesPopup {
    fn handle_key_events(
        &mut self,
        key: KeyEvent,
        focus_manager: &FocusManager,
    ) -> Result<(Vec<Action>, EventResult)> {
        if !focus_manager.has_focus(&self.focus_target()) {
            return Ok((vec![], EventResult::Ignored));
        }
        let send_back = match key.code {
            KeyCode::Enter => {
                if self.can_reset {
                    debug!("Got reset, sending Reset action and switching to Options");
                    vec![
                        Action::NodeTableActions(NodeTableActions::NodeManagementCommand(
                            NodeManagementCommand::ResetNodes,
                        )),
                        Action::SwitchScene(Scene::Options),
                    ]
                } else {
                    vec![]
                }
            }
            KeyCode::Esc => {
                debug!("Got Esc, switching to Options");
                vec![Action::SwitchScene(Scene::Options)]
            }
            KeyCode::Char(' ') => vec![],
            KeyCode::Backspace => {
                // if max limit reached, we should allow Backspace to work.
                self.confirmation_input_field.handle_event(&Event::Key(key));
                let input = self.confirmation_input_field.value().to_string();
                self.can_reset = input.to_lowercase() == "reset";
                vec![]
            }
            _ => {
                // max char limit
                if self.confirmation_input_field.value().chars().count() < INPUT_SIZE as usize {
                    self.confirmation_input_field.handle_event(&Event::Key(key));
                }
                let input = self.confirmation_input_field.value().to_string();
                self.can_reset = input.to_lowercase() == "reset";
                vec![]
            }
        };
        let result = if send_back.is_empty() {
            EventResult::Ignored
        } else {
            EventResult::Consumed
        };
        Ok((send_back, result))
    }

    fn focus_target(&self) -> FocusTarget {
        FocusTarget::ResetNodesPopup
    }

    fn update(&mut self, action: Action) -> Result<Option<Action>> {
        let send_back = match action {
            Action::SwitchScene(Scene::ResetNodesPopUp) => {
                self.confirmation_input_field = self
                    .confirmation_input_field
                    .clone()
                    .with_value(String::new());
                Some(Action::SwitchInputMode(InputMode::Entry))
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

        // layer zero
        let pop_up_border = Paragraph::new("").block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Reset Nodes ")
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
                Constraint::Length(4),
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

        let prompt = Paragraph::new("Type in 'reset' and press Enter to Reset all your nodes")
            .wrap(Wrap { trim: false })
            .block(Block::new().padding(Padding::horizontal(2)))
            .alignment(Alignment::Center)
            .fg(GHOST_WHITE);

        f.render_widget(prompt, layer_two[0]);

        let spaces =
            " ".repeat((INPUT_AREA - 1) as usize - self.confirmation_input_field.value().len());

        let input = Paragraph::new(Span::styled(
            format!("{}{} ", spaces, self.confirmation_input_field.value()),
            Style::default().fg(VIVID_SKY_BLUE).bg(INDIGO).underlined(),
        ))
        .alignment(Alignment::Center);

        f.render_widget(input, layer_two[1]);

        let text = Paragraph::new("This will clear out all the nodes and all the stored data. You should still keep all your earned rewards.")
            .wrap(Wrap { trim: false })
            .block(Block::new().padding(Padding::horizontal(2)))
            .alignment(Alignment::Center)
            .fg(GHOST_WHITE);
        f.render_widget(text, layer_two[2]);

        let dash = Block::new()
            .borders(Borders::BOTTOM)
            .border_style(Style::new().fg(GHOST_WHITE));
        f.render_widget(dash, layer_two[3]);

        let buttons_layer =
            Layout::horizontal(vec![Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(layer_two[4]);

        let button_no = Line::from(vec![Span::styled(
            "No, Cancel [Esc]",
            Style::default().fg(LIGHT_PERIWINKLE),
        )]);

        f.render_widget(
            Paragraph::new(button_no)
                .block(Block::default().padding(Padding::horizontal(2)))
                .alignment(Alignment::Left),
            buttons_layer[0],
        );

        let button_yes = Line::from(vec![Span::styled(
            "Reset Nodes [Enter]",
            if self.can_reset {
                Style::default().fg(EUCALYPTUS)
            } else {
                Style::default().fg(LIGHT_PERIWINKLE)
            },
        )])
        .alignment(Alignment::Right);

        f.render_widget(
            Paragraph::new(button_yes)
                .block(Block::default().padding(Padding::horizontal(2)))
                .alignment(Alignment::Right),
            buttons_layer[1],
        );

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

    fn build_popup() -> ResetNodesPopup {
        ResetNodesPopup::default()
    }

    #[test]
    fn typing_reset_enables_confirmation() {
        let mut popup = build_popup();
        popup.confirmation_input_field = Input::default();
        let focus_manager = FocusManager::new(FocusTarget::ResetNodesPopup);
        for ch in ['r', 'e', 's', 'e', 't'] {
            let _ = popup
                .handle_key_events(
                    KeyEvent::new(KeyCode::Char(ch), KeyModifiers::empty()),
                    &focus_manager,
                )
                .expect("handled");
        }
        assert!(popup.can_reset);
        let (actions, result) = popup
            .handle_key_events(
                KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()),
                &focus_manager,
            )
            .expect("enter handled");
        assert_eq!(result, EventResult::Consumed);
        assert!(actions.iter().any(|a| matches!(
            a,
            Action::NodeTableActions(NodeTableActions::NodeManagementCommand(
                NodeManagementCommand::ResetNodes
            ))
        )));
        assert!(
            actions
                .iter()
                .any(|a| matches!(a, Action::SwitchScene(Scene::Options)))
        );
    }

    #[test]
    fn handle_key_events_requires_focus() {
        let mut popup = build_popup();
        let focus_manager = FocusManager::new(FocusTarget::Status);
        let (actions, result) = popup
            .handle_key_events(
                KeyEvent::new(KeyCode::Char('r'), KeyModifiers::empty()),
                &focus_manager,
            )
            .expect("handled");
        assert!(actions.is_empty());
        assert_eq!(result, EventResult::Ignored);
    }

    #[test]
    fn escape_returns_to_options_without_modifying_input() {
        let mut popup = build_popup();
        popup.confirmation_input_field = Input::default().with_value("reset".into());
        let focus_manager = FocusManager::new(FocusTarget::ResetNodesPopup);
        let (actions, _) = popup
            .handle_key_events(
                KeyEvent::new(KeyCode::Esc, KeyModifiers::empty()),
                &focus_manager,
            )
            .expect("handled");
        assert!(actions.contains(&Action::SwitchScene(Scene::Options)));
        assert_eq!(popup.confirmation_input_field.value(), "reset");
    }

    #[test]
    fn update_switch_scene_prepares_entry_mode() {
        let mut popup = build_popup();
        popup.confirmation_input_field = Input::default().with_value("something".into());
        let action = popup
            .update(Action::SwitchScene(Scene::ResetNodesPopUp))
            .expect("update")
            .expect("action");
        assert_eq!(action, Action::SwitchInputMode(InputMode::Entry));
        assert!(popup.confirmation_input_field.value().is_empty());
    }

    #[test]
    fn wrong_confirmation_text_prevents_reset() {
        let mut popup = build_popup();
        popup.confirmation_input_field = Input::default();
        let focus_manager = FocusManager::new(FocusTarget::ResetNodesPopup);

        // Type wrong confirmation
        for ch in "wrong".chars() {
            let _ = popup
                .handle_key_events(
                    KeyEvent::new(KeyCode::Char(ch), KeyModifiers::empty()),
                    &focus_manager,
                )
                .expect("char handled");
        }

        // Try to confirm with Enter
        let (actions, result) = popup
            .handle_key_events(
                KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()),
                &focus_manager,
            )
            .expect("enter handled");

        // Validate error state - Enter should be ignored when no actions are emitted
        assert_eq!(result, EventResult::Ignored);
        assert!(
            !popup.can_reset,
            "reset should not be enabled with wrong confirmation"
        );
        assert_eq!(
            popup.confirmation_input_field.value(),
            "wrong",
            "input should retain wrong confirmation text"
        );
        assert!(
            actions.is_empty(),
            "no actions should be emitted when confirmation is wrong"
        );
    }
}
