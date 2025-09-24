// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::{
    components::utils::centered_rect_fixed,
    style::{EUCALYPTUS, GHOST_WHITE, RED, clear_area},
    tui::Frame,
};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, Padding, Paragraph, Wrap},
};

/// Error popup is a popup that is used to display error messages to the user.
///
/// It accepts a title, a message and an error message.
/// Handles key events to hide the popup (Enter and Esc keys).
///
/// How to use:
/// 1. Create a new ErrorPopup member in your component.
/// 2. Show the error popup by calling the `show` method.
/// 3. Hide the error popup by calling the `hide` method.
/// 4. Check if the error popup is visible by calling the `is_visible` method.
/// 5. Draw the error popup by calling the `draw_error` method in your `draw` function.
/// 6. Handle the input for the error popup by calling the `handle_input` method.
///
/// How to trigger the error
///
/// ```ignore
/// self.error_popup = Some(ErrorPopup::new(
///     "Error".to_string(),
///     "This is a test error message".to_string(),
///     "raw message".to_string(),
/// ));
/// if let Some(error_popup) = &mut self.error_popup {
///     error_popup.show();
/// }
/// ```

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ErrorPopup {
    visible: bool,
    title: String,
    message: String,
    error_message: String,
}

impl ErrorPopup {
    pub fn new(title: &str, message: &str, error_message: &str) -> Self {
        Self {
            visible: false,
            title: title.to_string(),
            message: message.to_string(),
            error_message: error_message.to_string(),
        }
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn error_message(&self) -> &str {
        &self.error_message
    }

    pub fn draw_error(&self, f: &mut Frame, area: Rect) {
        if !self.visible {
            return;
        }

        let layer_zero = centered_rect_fixed(52, 15, area);

        let layer_one = Layout::new(
            Direction::Vertical,
            [
                // for the pop_up_border + padding
                Constraint::Length(2),
                // for the text
                Constraint::Min(1),
                // for the pop_up_border
                Constraint::Length(1),
            ],
        )
        .split(layer_zero);

        let pop_up_border = Paragraph::new("").block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(" {} ", self.title))
                .bold()
                .title_style(Style::new().fg(RED))
                .padding(Padding::uniform(2))
                .border_style(Style::new().fg(RED)),
        );
        clear_area(f, layer_zero);

        let layer_two = Layout::new(
            Direction::Vertical,
            [
                // for the message
                Constraint::Length(4),
                // for the error_message
                Constraint::Length(7),
                // gap
                Constraint::Length(1),
                // for the buttons
                Constraint::Length(1),
            ],
        )
        .split(layer_one[1]);

        let prompt = Paragraph::new(self.message.clone())
            .block(
                Block::default()
                    .padding(Padding::horizontal(2))
                    .padding(Padding::vertical(1)),
            )
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true });

        f.render_widget(prompt.fg(GHOST_WHITE), layer_two[0]);

        let text = Paragraph::new(self.error_message.clone())
            .block(Block::default().padding(Padding::horizontal(2)))
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true });
        f.render_widget(text.fg(GHOST_WHITE), layer_two[1]);

        let dash = Block::new()
            .borders(Borders::BOTTOM)
            .border_style(Style::new().fg(GHOST_WHITE));
        f.render_widget(dash, layer_two[2]);

        let buttons_layer =
            Layout::horizontal(vec![Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(layer_two[3]);
        let button_ok = Line::from(vec![
            Span::styled("OK ", Style::default().fg(EUCALYPTUS)),
            Span::styled("[Enter]   ", Style::default().fg(GHOST_WHITE)),
        ])
        .alignment(Alignment::Right);

        f.render_widget(button_ok, buttons_layer[1]);

        // We render now so the borders are on top of the other widgets
        f.render_widget(pop_up_border, layer_zero);
    }

    pub fn handle_input(&mut self, key: KeyEvent) -> bool {
        if self.visible && (key.code == KeyCode::Esc || key.code == KeyCode::Enter) {
            self.hide();
            true
        } else {
            false
        }
    }

    pub fn show(&mut self) {
        debug!("Showing error popup");
        self.visible = true;
    }

    pub fn hide(&mut self) {
        debug!("Hiding error popup");
        self.visible = false;
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    #[test]
    fn test_error_popup_handle_input_enter_key() {
        let mut error_popup = ErrorPopup::new("Error", "Message", "Details");
        error_popup.show();

        let key_event = KeyEvent::new(KeyCode::Enter, KeyModifiers::empty());
        let handled = error_popup.handle_input(key_event);

        assert!(handled);
        assert!(!error_popup.is_visible());
    }

    #[test]
    fn test_error_popup_handle_input_esc_key() {
        let mut error_popup = ErrorPopup::new("Error", "Message", "Details");
        error_popup.show();

        let key_event = KeyEvent::new(KeyCode::Esc, KeyModifiers::empty());
        let handled = error_popup.handle_input(key_event);

        assert!(handled);
        assert!(!error_popup.is_visible());
    }

    #[test]
    fn test_error_popup_lifecycle_simulation() {
        let mut error_popup = ErrorPopup::new(
            "Lifecycle Test",
            "Testing full lifecycle",
            "Complete error lifecycle test",
        );

        // 1. Created - not visible
        assert!(!error_popup.is_visible());

        // 2. Show the error
        error_popup.show();
        assert!(error_popup.is_visible());

        // 3. User presses various keys (ignored)
        let ignored_keys = [KeyCode::Char('x'), KeyCode::Up, KeyCode::Tab];
        for key_code in ignored_keys {
            let key_event = KeyEvent::new(key_code, KeyModifiers::empty());
            let handled = error_popup.handle_input(key_event);
            assert!(!handled);
            assert!(error_popup.is_visible());
        }

        // 4. User presses Enter to dismiss
        let key_event = KeyEvent::new(KeyCode::Enter, KeyModifiers::empty());
        let handled = error_popup.handle_input(key_event);
        assert!(handled);
        assert!(!error_popup.is_visible());

        // 5. Show again
        error_popup.show();
        assert!(error_popup.is_visible());

        // 6. User presses Esc to dismiss
        let key_event = KeyEvent::new(KeyCode::Esc, KeyModifiers::empty());
        let handled = error_popup.handle_input(key_event);
        assert!(handled);
        assert!(!error_popup.is_visible());
    }
}
