// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use ratatui::widgets::TableState;
use throbber_widgets_tui::ThrobberState;

#[derive(Default, Clone)]
pub struct StatefulTable<T> {
    pub state: TableState,
    pub items: Vec<T>,
    pub last_selected: Option<usize>,
}

impl<T> StatefulTable<T> {
    pub fn with_items(items: Vec<T>) -> Self {
        StatefulTable {
            state: TableState::default(),
            items,
            last_selected: None,
        }
    }

    pub fn next(&mut self) {
        let i = match self.state.selected() {
            Some(i) => {
                if !self.items.is_empty() {
                    if i >= self.items.len() - 1 { 0 } else { i + 1 }
                } else {
                    0
                }
            }
            None => self.last_selected.unwrap_or(0),
        };
        self.state.select(Some(i));
        self.last_selected = Some(i);
    }

    pub fn previous(&mut self) {
        let i = match self.state.selected() {
            Some(i) => {
                if !self.items.is_empty() {
                    if i == 0 { self.items.len() - 1 } else { i - 1 }
                } else {
                    0
                }
            }
            None => self.last_selected.unwrap_or(0),
        };
        self.state.select(Some(i));
        self.last_selected = Some(i);
    }

    pub fn selected_item(&self) -> Option<&T> {
        self.state
            .selected()
            .and_then(|index| self.items.get(index))
    }

    pub fn selected_item_mut(&mut self) -> Option<&mut T> {
        self.state
            .selected()
            .and_then(|index| self.items.get_mut(index))
    }
}

#[derive(Default, Clone)]
pub struct TableUiState {
    spinner_states: Vec<ThrobberState>,
}

impl TableUiState {
    pub fn new() -> Self {
        Self {
            spinner_states: Vec::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.spinner_states.len()
    }

    pub fn is_empty(&self) -> bool {
        self.spinner_states.is_empty()
    }

    pub fn spinner_states(&self) -> &[ThrobberState] {
        &self.spinner_states
    }

    pub fn spinner_states_mut(&mut self) -> &mut Vec<ThrobberState> {
        &mut self.spinner_states
    }

    pub fn ensure_spinner_count(&mut self, count: usize) {
        self.spinner_states
            .resize_with(count, ThrobberState::default);
    }
}
