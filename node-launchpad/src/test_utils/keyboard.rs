// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Builder for creating sequences of keyboard events
pub struct KeySequence {
    events: Vec<KeyEvent>,
}

impl KeySequence {
    /// Create a new empty key sequence
    pub fn new() -> Self {
        Self { events: Vec::new() }
    }

    /// Add a regular key press
    pub fn key(mut self, c: char) -> Self {
        self.events
            .push(KeyEvent::new(KeyCode::Char(c), KeyModifiers::empty()));
        self
    }

    /// Add a raw KeyEvent
    pub fn push_event(mut self, event: KeyEvent) -> Self {
        self.events.push(event);
        self
    }

    /// Add a key with Ctrl modifier
    pub fn ctrl(mut self, c: char) -> Self {
        self.events
            .push(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL));
        self
    }

    /// Add a key with Alt modifier  
    pub fn alt(mut self, c: char) -> Self {
        self.events
            .push(KeyEvent::new(KeyCode::Char(c), KeyModifiers::ALT));
        self
    }

    /// Add a key with Shift modifier
    pub fn shift(mut self, c: char) -> Self {
        self.events
            .push(KeyEvent::new(KeyCode::Char(c), KeyModifiers::SHIFT));
        self
    }

    /// Add ESC key
    pub fn esc(mut self) -> Self {
        self.events
            .push(KeyEvent::new(KeyCode::Esc, KeyModifiers::empty()));
        self
    }

    /// Add Enter key
    pub fn enter(mut self) -> Self {
        self.events
            .push(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));
        self
    }

    /// Add Tab key
    pub fn tab(mut self) -> Self {
        self.events
            .push(KeyEvent::new(KeyCode::Tab, KeyModifiers::empty()));
        self
    }

    /// Add Backspace key
    pub fn backspace(mut self) -> Self {
        self.events
            .push(KeyEvent::new(KeyCode::Backspace, KeyModifiers::empty()));
        self
    }

    /// Add Delete key
    pub fn delete(mut self) -> Self {
        self.events
            .push(KeyEvent::new(KeyCode::Delete, KeyModifiers::empty()));
        self
    }

    /// Add Down arrow key
    pub fn arrow_down(mut self) -> Self {
        self.events
            .push(KeyEvent::new(KeyCode::Down, KeyModifiers::empty()));
        self
    }

    /// Add Up arrow key
    pub fn arrow_up(mut self) -> Self {
        self.events
            .push(KeyEvent::new(KeyCode::Up, KeyModifiers::empty()));
        self
    }

    /// Add Left arrow key
    pub fn arrow_left(mut self) -> Self {
        self.events
            .push(KeyEvent::new(KeyCode::Left, KeyModifiers::empty()));
        self
    }

    /// Add Right arrow key
    pub fn arrow_right(mut self) -> Self {
        self.events
            .push(KeyEvent::new(KeyCode::Right, KeyModifiers::empty()));
        self
    }

    /// Add Page Down key
    pub fn page_down(mut self) -> Self {
        self.events
            .push(KeyEvent::new(KeyCode::PageDown, KeyModifiers::empty()));
        self
    }

    /// Add Page Up key
    pub fn page_up(mut self) -> Self {
        self.events
            .push(KeyEvent::new(KeyCode::PageUp, KeyModifiers::empty()));
        self
    }

    /// Add Home key
    pub fn home(mut self) -> Self {
        self.events
            .push(KeyEvent::new(KeyCode::Home, KeyModifiers::empty()));
        self
    }

    /// Add End key
    pub fn end(mut self) -> Self {
        self.events
            .push(KeyEvent::new(KeyCode::End, KeyModifiers::empty()));
        self
    }

    /// Add a string of characters as individual key events
    pub fn string(mut self, text: &str) -> Self {
        for c in text.chars() {
            self.events
                .push(KeyEvent::new(KeyCode::Char(c), KeyModifiers::empty()));
        }
        self
    }

    /// Repeat a key n times
    pub fn repeat(mut self, key_code: KeyCode, count: usize) -> Self {
        for _ in 0..count {
            self.events
                .push(KeyEvent::new(key_code, KeyModifiers::empty()));
        }
        self
    }

    /// Build the sequence into a vector of KeyEvents
    pub fn build(self) -> Vec<KeyEvent> {
        self.events
    }
}

impl Default for KeySequence {
    fn default() -> Self {
        Self::new()
    }
}
