// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::{
    action::Action,
    components::{Component, utils::centered_rect},
    focus::{EventResult, FocusManager, FocusTarget},
    mode::Scene,
    style::{EUCALYPTUS, GHOST_WHITE, LIGHT_PERIWINKLE, RED, VERY_LIGHT_AZURE, clear_area},
    tui::Frame,
};
use ant_node_manager::config::get_service_log_dir_path;
use ant_releases::ReleaseType;
use color_eyre::Result;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Style, Stylize},
    text::{Line, Span},
    widgets::{
        Block, Borders, List, ListItem, ListState, Padding, Paragraph, Scrollbar,
        ScrollbarOrientation, ScrollbarState,
    },
};
use std::{
    fs,
    io::{BufRead, BufReader},
};

#[derive(Default)]
pub struct NodeLogsPopup {
    node_name: String,
    logs: Vec<String>,
    list_state: ListState,
    scroll_state: ScrollbarState,
    is_following_tail: bool,
}

impl NodeLogsPopup {
    pub fn new(node_name: String) -> Self {
        let mut instance = Self {
            node_name,
            logs: vec![],
            list_state: ListState::default(),
            scroll_state: ScrollbarState::default(),
            is_following_tail: true,
        };
        // Load initial logs
        if let Err(e) = instance.load_logs() {
            log::error!("Failed to load logs for node: {e}");
            instance.logs = vec![format!("Error loading logs: {e}")];
        }
        instance
    }

    fn load_logs(&mut self) -> Result<()> {
        if self.node_name.is_empty() || self.node_name == "No node available" {
            self.logs = vec![
                "No nodes available for log viewing".to_string(),
                "".to_string(),
                "To view logs:".to_string(),
                "1. Add some nodes by pressing [+]".to_string(),
                "2. Start at least one node".to_string(),
                "3. Select a node and press [L] to view its logs".to_string(),
            ];
            return Ok(());
        }

        let log_dir =
            get_service_log_dir_path(ReleaseType::NodeLaunchpad, None, None)?.join(&self.node_name);

        if !log_dir.exists() {
            self.logs = vec![
                format!("Log directory not found for node '{}'", self.node_name),
                format!("Expected path: {}", log_dir.display()),
                "".to_string(),
                "This could mean:".to_string(),
                "- The node hasn't been started yet".to_string(),
                "- The node name is incorrect".to_string(),
                "- Logs are stored in a different location".to_string(),
            ];
            return Ok(());
        }

        // Find the most recent log file
        let mut log_files: Vec<_> = fs::read_dir(&log_dir)?
            .filter_map(|entry| {
                let entry = entry.ok()?;
                let path = entry.path();
                if path.is_file() && path.extension()? == "log" {
                    let metadata = entry.metadata().ok()?;
                    Some((path, metadata.modified().ok()?))
                } else {
                    None
                }
            })
            .collect();
        if log_files.is_empty() {
            self.logs = vec![
                format!("No log files found for node '{}'", self.node_name),
                format!("Searched in: {}", log_dir.display()),
            ];
            return Ok(());
        }

        // Sort by modification time, most recent first
        log_files.sort_by(|a, b| b.1.cmp(&a.1));
        let latest_log_file = &log_files[0].0;

        // Read the log file (tail the last 1000 lines for performance)
        let file = fs::File::open(latest_log_file)?;
        let reader = BufReader::new(file);
        let mut lines: Vec<String> = reader.lines().collect::<Result<Vec<_>, _>>()?;

        // Keep only the last 1000 lines for performance
        if lines.len() > 1000 {
            let skip_count = lines.len() - 1000;
            lines = lines.into_iter().skip(skip_count).collect();
        }

        if lines.is_empty() {
            self.logs = vec![
                format!("Log file for node '{}' is empty", self.node_name),
                format!("File: {}", latest_log_file.display()),
            ];
        } else {
            self.logs = lines;
            // Add header with file info
            self.logs
                .insert(0, format!("=== Logs for node '{}' ===", self.node_name));
            self.logs
                .insert(1, format!("File: {}", latest_log_file.display()));
            self.logs.insert(
                2,
                format!("Lines: {} (showing last 1000)", self.logs.len() - 3),
            );
            self.logs.insert(3, "".to_string());
        }

        if self.is_following_tail && !self.logs.is_empty() {
            let last_index = self.logs.len() - 1;
            self.list_state.select(Some(last_index));
            self.scroll_state = self.scroll_state.position(last_index);
        }
        self.scroll_state = self.scroll_state.content_length(self.logs.len());

        Ok(())
    }

    pub fn set_node_name(&mut self, node_name: String) {
        if self.node_name != node_name {
            self.node_name = node_name;
            // Reload logs for the new node
            if let Err(e) = self.load_logs() {
                log::error!("Failed to load logs for node: {e}");
                self.logs = vec![format!("Error loading logs: {e}")];
            }
        }
    }

    pub fn add_log_line(&mut self, line: String) {
        self.logs.push(line);
        if self.is_following_tail {
            // Auto-scroll to bottom
            let last_index = self.logs.len().saturating_sub(1);
            self.list_state.select(Some(last_index));
            self.scroll_state = self.scroll_state.position(last_index);
        }
    }

    pub fn set_logs(&mut self, logs: Vec<String>) {
        self.logs = logs;
        if self.is_following_tail && !self.logs.is_empty() {
            let last_index = self.logs.len().saturating_sub(1);
            self.list_state.select(Some(last_index));
            self.scroll_state = self.scroll_state.position(last_index);
        }
        self.scroll_state = self.scroll_state.content_length(self.logs.len());
    }

    fn handle_scroll_up(&mut self) {
        self.is_following_tail = false;
        if let Some(selected) = self.list_state.selected() {
            if selected > 0 {
                self.list_state.select(Some(selected - 1));
                self.scroll_state = self.scroll_state.position(selected - 1);
            }
        } else if !self.logs.is_empty() {
            self.list_state.select(Some(self.logs.len() - 1));
        }
    }

    fn handle_scroll_down(&mut self) {
        if let Some(selected) = self.list_state.selected() {
            if selected < self.logs.len().saturating_sub(1) {
                let new_pos = selected + 1;
                self.list_state.select(Some(new_pos));
                self.scroll_state = self.scroll_state.position(new_pos);

                // If we've reached the bottom, enable tail following
                if new_pos == self.logs.len().saturating_sub(1) {
                    self.is_following_tail = true;
                }
            } else {
                // Already at the bottom, enable tail following
                self.is_following_tail = true;
            }
        } else if !self.logs.is_empty() {
            self.list_state.select(Some(0));
            self.scroll_state = self.scroll_state.position(0);
        }
    }

    fn handle_page_up(&mut self) {
        self.is_following_tail = false;
        if let Some(selected) = self.list_state.selected() {
            let new_pos = selected.saturating_sub(10);
            self.list_state.select(Some(new_pos));
            self.scroll_state = self.scroll_state.position(new_pos);
        }
    }

    fn handle_page_down(&mut self) {
        if let Some(selected) = self.list_state.selected() {
            let new_pos = (selected + 10).min(self.logs.len().saturating_sub(1));
            self.list_state.select(Some(new_pos));
            self.scroll_state = self.scroll_state.position(new_pos);

            if new_pos >= self.logs.len().saturating_sub(1) {
                self.is_following_tail = true;
            }
        }
    }

    fn handle_home(&mut self) {
        self.is_following_tail = false;
        if !self.logs.is_empty() {
            self.list_state.select(Some(0));
            self.scroll_state = self.scroll_state.position(0);
        }
    }

    fn handle_end(&mut self) {
        if !self.logs.is_empty() {
            let last_index = self.logs.len() - 1;
            self.list_state.select(Some(last_index));
            self.scroll_state = self.scroll_state.position(last_index);
            self.is_following_tail = true;
        }
    }
}

impl Component for NodeLogsPopup {
    fn focus_target(&self) -> FocusTarget {
        FocusTarget::NodeLogsPopup
    }

    fn handle_key_events(
        &mut self,
        key: KeyEvent,
        _focus_manager: &FocusManager,
    ) -> Result<(Vec<Action>, EventResult)> {
        let action = match key.code {
            KeyCode::Esc => Action::SwitchScene(Scene::Status),
            KeyCode::Up => {
                self.handle_scroll_up();
                return Ok((vec![], EventResult::Consumed));
            }
            KeyCode::Down => {
                self.handle_scroll_down();
                return Ok((vec![], EventResult::Consumed));
            }
            KeyCode::PageUp => {
                self.handle_page_up();
                return Ok((vec![], EventResult::Consumed));
            }
            KeyCode::PageDown => {
                self.handle_page_down();
                return Ok((vec![], EventResult::Consumed));
            }
            KeyCode::Home => {
                self.handle_home();
                return Ok((vec![], EventResult::Consumed));
            }
            KeyCode::End => {
                self.handle_end();
                return Ok((vec![], EventResult::Consumed));
            }
            _ => return Ok((vec![], EventResult::Ignored)),
        };

        Ok((vec![action], EventResult::Consumed))
    }

    fn update(&mut self, action: Action) -> Result<Option<Action>> {
        match action {
            Action::SwitchScene(Scene::NodeLogsPopUp) => {
                Ok(Some(Action::SwitchInputMode(crate::mode::InputMode::Entry)))
            }
            Action::SetNodeLogsTarget(node_name) => {
                self.set_node_name(node_name);
                Ok(None)
            }
            _ => Ok(None),
        }
    }

    fn draw(&mut self, f: &mut Frame<'_>, area: Rect) -> Result<()> {
        // Create a popup area (80% width, 85% height)
        let popup_area = centered_rect(80, 85, area);
        clear_area(f, popup_area);

        // Create the main layout
        let main_layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // Title
                Constraint::Min(1),    // Logs
                Constraint::Length(5), // Instructions (2 lines + padding + border)
            ])
            .split(popup_area);

        // Draw border and title
        let title = format!(" Node Logs - {} ", self.node_name);
        let block = Block::default()
            .borders(Borders::ALL)
            .title(title)
            .title_style(Style::default().fg(EUCALYPTUS).bold())
            .border_style(Style::default().fg(EUCALYPTUS));

        f.render_widget(block, popup_area);

        // Create logs display area
        let logs_area = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(1), Constraint::Length(1)])
            .margin(1)
            .split(main_layout[1]);

        // Convert logs to ListItems
        let log_items: Vec<ListItem> = self
            .logs
            .iter()
            .enumerate()
            .map(|(i, log)| {
                let style = if Some(i) == self.list_state.selected() {
                    Style::default().fg(GHOST_WHITE).bg(VERY_LIGHT_AZURE)
                } else {
                    Style::default().fg(GHOST_WHITE)
                };
                ListItem::new(log.clone()).style(style)
            })
            .collect();

        let logs_list = List::new(log_items)
            .style(Style::default().fg(GHOST_WHITE))
            .highlight_style(Style::default().fg(GHOST_WHITE).bg(VERY_LIGHT_AZURE));

        f.render_stateful_widget(logs_list, logs_area[0], &mut self.list_state);

        // Draw scrollbar
        let scrollbar = Scrollbar::default()
            .orientation(ScrollbarOrientation::VerticalRight)
            .style(Style::default().fg(LIGHT_PERIWINKLE));

        f.render_stateful_widget(scrollbar, logs_area[1], &mut self.scroll_state);

        // Draw instructions
        let instructions = Paragraph::new(vec![
            Line::from(vec![
                Span::styled("↑/↓", Style::default().fg(EUCALYPTUS).bold()),
                Span::styled(" Scroll  ", Style::default().fg(GHOST_WHITE)),
                Span::styled("PgUp/PgDn", Style::default().fg(EUCALYPTUS).bold()),
                Span::styled(" Page  ", Style::default().fg(GHOST_WHITE)),
                Span::styled("Home/End", Style::default().fg(EUCALYPTUS).bold()),
                Span::styled(" Jump", Style::default().fg(GHOST_WHITE)),
            ]),
            Line::from(vec![
                Span::styled("ESC", Style::default().fg(RED).bold()),
                Span::styled(" Close  ", Style::default().fg(GHOST_WHITE)),
                Span::styled("[TAIL]", Style::default().fg(EUCALYPTUS).bold()),
                Span::styled(" Auto-follow at bottom", Style::default().fg(GHOST_WHITE)),
            ]),
        ])
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .borders(Borders::TOP)
                .border_style(Style::default().fg(EUCALYPTUS))
                .padding(Padding::uniform(1)),
        );

        f.render_widget(instructions, main_layout[2]);

        // Draw tail following indicator
        if self.is_following_tail {
            let indicator_area = Rect::new(popup_area.right() - 12, popup_area.y + 1, 11, 1);
            let indicator =
                Paragraph::new(" [TAIL] ").style(Style::default().fg(EUCALYPTUS).bold());
            f.render_widget(indicator, indicator_area);
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        focus::{EventResult, FocusManager, FocusTarget},
        mode::Scene,
        test_utils::*,
    };
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::{Terminal, backend::TestBackend};

    #[test]
    fn test_esc_key_closes_popup() {
        let mut popup = NodeLogsPopup::new("antnode1".to_string());
        let focus_manager = FocusManager::new(FocusTarget::NodeLogsPopup);
        let key_event = KeyEvent::new(KeyCode::Esc, KeyModifiers::empty());

        let result = popup.handle_key_events(key_event, &focus_manager);

        assert!(result.is_ok());
        let (actions, event_result) = result.unwrap();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0], Action::SwitchScene(Scene::Status));
        assert_eq!(event_result, EventResult::Consumed);
    }

    #[test]
    fn test_page_up_key_handling() {
        let mut popup = NodeLogsPopup::new("test_node".to_string());
        let focus_manager = FocusManager::new(FocusTarget::NodeLogsPopup);
        let key_event = KeyEvent::new(KeyCode::PageUp, KeyModifiers::empty());

        let result = popup.handle_key_events(key_event, &focus_manager);

        assert!(result.is_ok());
        let (actions, event_result) = result.unwrap();
        assert_eq!(actions.len(), 0);
        assert_eq!(event_result, EventResult::Consumed);
    }

    #[test]
    fn test_keyboard_sequence_simulation() {
        let mut popup = NodeLogsPopup::new("test_node".to_string());
        let focus_manager = FocusManager::new(FocusTarget::NodeLogsPopup);

        let key_sequence = KeySequence::new()
            .arrow_down()
            .arrow_down()
            .arrow_up()
            .page_down()
            .home()
            .end()
            .esc()
            .build();

        for key_event in key_sequence {
            let result = popup.handle_key_events(key_event, &focus_manager);
            assert!(result.is_ok());

            if key_event.code == KeyCode::Esc {
                let (actions, _) = result.unwrap();
                assert_eq!(actions.len(), 1);
                assert_eq!(actions[0], Action::SwitchScene(Scene::Status));
                break;
            }
        }
    }

    // === ADVANCED TESTING ===

    #[test]
    fn test_tail_mode_behavior_with_scrolling() {
        let mut popup = NodeLogsPopup::new("test_node".to_string());
        let test_logs = (0..10).map(|i| format!("Log line {i}")).collect();
        popup.set_logs(test_logs);

        // Should start in tail mode at the end
        assert!(popup.is_following_tail);
        assert_eq!(popup.list_state.selected(), Some(9));

        // Scrolling up should disable tail mode
        popup.handle_scroll_up();
        assert!(!popup.is_following_tail);
        assert_eq!(popup.list_state.selected(), Some(8));

        // Scrolling down to the end should re-enable tail mode
        popup.handle_scroll_down();
        assert!(popup.is_following_tail);
        assert_eq!(popup.list_state.selected(), Some(9));

        // Page up should disable tail mode
        popup.handle_page_up();
        assert!(!popup.is_following_tail);

        // Page down to the end should re-enable tail mode
        popup.handle_page_down();
        assert!(popup.is_following_tail);

        // Home should disable tail mode
        popup.handle_home();
        assert!(!popup.is_following_tail);
        assert_eq!(popup.list_state.selected(), Some(0));

        // End should re-enable tail mode
        popup.handle_end();
        assert!(popup.is_following_tail);
        assert_eq!(popup.list_state.selected(), Some(9));
    }

    #[test]
    fn test_set_logs_functionality() {
        let mut popup = NodeLogsPopup::new("test_node".to_string());
        let test_logs = vec![
            "First log line".to_string(),
            "Second log line".to_string(),
            "Third log line".to_string(),
        ];

        popup.set_logs(test_logs.clone());

        assert_eq!(popup.logs, test_logs);
        // Should auto-scroll to bottom when in tail mode
        assert_eq!(popup.list_state.selected(), Some(2));
    }

    #[test]
    fn test_set_logs_empty_collection() {
        let mut popup = NodeLogsPopup::new("test_node".to_string());
        popup.set_logs(vec![]);

        assert!(popup.logs.is_empty());
        assert_eq!(popup.list_state.selected(), None);
    }

    #[test]
    fn test_add_log_line_functionality() {
        let mut popup = NodeLogsPopup::new("test_node".to_string());
        popup.set_logs(vec!["Initial log".to_string()]);

        // Add a new log line
        popup.add_log_line("New log line".to_string());

        assert_eq!(popup.logs.len(), 2);
        assert_eq!(popup.logs[1], "New log line");

        // Should auto-scroll to new line when in tail mode
        assert!(popup.is_following_tail);
        assert_eq!(popup.list_state.selected(), Some(1));
    }

    #[test]
    fn test_add_log_line_when_not_following_tail() {
        let mut popup = NodeLogsPopup::new("test_node".to_string());
        popup.set_logs(vec![
            "Line 1".to_string(),
            "Line 2".to_string(),
            "Line 3".to_string(),
        ]);

        // Disable tail following by scrolling up
        popup.handle_scroll_up();
        assert!(!popup.is_following_tail);
        let selected_before = popup.list_state.selected();

        // Add a new log line
        popup.add_log_line("New line".to_string());

        assert_eq!(popup.logs.len(), 4);
        assert_eq!(popup.logs[3], "New line");

        // Should NOT auto-scroll when not following tail
        assert_eq!(popup.list_state.selected(), selected_before);
    }

    #[test]
    fn test_no_node_available_message() {
        let popup = NodeLogsPopup::new("No node available".to_string());

        // Should display specific messages for no nodes
        assert!(
            popup
                .logs
                .iter()
                .any(|log| log.contains("No nodes available for log viewing"))
        );
        assert!(
            popup
                .logs
                .iter()
                .any(|log| log.contains("Add some nodes by pressing [+]"))
        );
        assert!(
            popup
                .logs
                .iter()
                .any(|log| log.contains("Select a node and press [L] to view its logs"))
        );
    }

    #[test]
    fn test_empty_node_name_message() {
        let popup = NodeLogsPopup::new("".to_string());

        // Should display no nodes message for empty name
        assert!(
            popup
                .logs
                .iter()
                .any(|log| log.contains("No nodes available for log viewing"))
        );
    }

    #[test]
    fn test_set_node_name_changes_logs() {
        let mut popup = NodeLogsPopup::new("initial_node".to_string());
        assert_eq!(popup.node_name, "initial_node");

        // Change to a different node name
        popup.set_node_name("new_node".to_string());
        assert_eq!(popup.node_name, "new_node");

        // Set to same name should not reload
        popup.set_node_name("new_node".to_string());
        assert_eq!(popup.node_name, "new_node");

        // Change to "No node available" should trigger special message
        popup.set_node_name("No node available".to_string());
        assert!(
            popup
                .logs
                .iter()
                .any(|log| log.contains("No nodes available for log viewing"))
        );
    }

    #[test]
    fn test_scroll_state_synchronization() {
        let mut popup = NodeLogsPopup::new("test_node".to_string());
        let test_logs: Vec<String> = (0..20).map(|i| format!("Log line {i}")).collect();
        popup.set_logs(test_logs);

        // Scroll to different positions and verify list state is updated
        popup.handle_home();
        assert_eq!(popup.list_state.selected(), Some(0));

        popup.handle_page_down();
        let selected = popup.list_state.selected().unwrap();
        assert!(selected > 0); // Should have moved from position 0

        popup.handle_end();
        assert_eq!(popup.list_state.selected(), Some(19));
    }

    #[test]
    fn test_scroll_navigation_with_empty_logs() {
        let mut popup = NodeLogsPopup::new("test_node".to_string());
        popup.set_logs(vec![]);

        // All navigation should be safe with empty logs
        popup.handle_scroll_up();
        assert_eq!(popup.list_state.selected(), None);

        popup.handle_scroll_down();
        assert_eq!(popup.list_state.selected(), None);

        popup.handle_page_up();
        assert_eq!(popup.list_state.selected(), None);

        popup.handle_page_down();
        assert_eq!(popup.list_state.selected(), None);

        popup.handle_home();
        assert_eq!(popup.list_state.selected(), None);

        popup.handle_end();
        assert_eq!(popup.list_state.selected(), None);
    }

    #[test]
    fn test_scroll_navigation_with_single_log() {
        let mut popup = NodeLogsPopup::new("test_node".to_string());
        popup.set_logs(vec!["Single log line".to_string()]);

        // Should start at the only item
        assert_eq!(popup.list_state.selected(), Some(0));

        // All navigation should keep selection at the single item
        popup.handle_scroll_up();
        assert_eq!(popup.list_state.selected(), Some(0));

        popup.handle_scroll_down();
        assert_eq!(popup.list_state.selected(), Some(0));

        popup.handle_page_up();
        assert_eq!(popup.list_state.selected(), Some(0));

        popup.handle_page_down();
        assert_eq!(popup.list_state.selected(), Some(0));
    }

    #[test]
    fn test_page_navigation_behavior() {
        let mut popup = NodeLogsPopup::new("test_node".to_string());
        let test_logs: Vec<String> = (0..50).map(|i| format!("Log line {i}")).collect();
        popup.set_logs(test_logs);

        // Start at bottom (index 49)
        assert_eq!(popup.list_state.selected(), Some(49));

        // Page up should move up by 10
        popup.handle_page_up();
        assert_eq!(popup.list_state.selected(), Some(39));
        assert!(!popup.is_following_tail);

        // Page down should move down by 10
        popup.handle_page_down();
        assert_eq!(popup.list_state.selected(), Some(49));
        assert!(popup.is_following_tail); // Should re-enable tail at bottom

        // From top, page up should stay at 0
        popup.handle_home();
        popup.handle_page_up();
        assert_eq!(popup.list_state.selected(), Some(0));
    }

    #[test]
    fn test_log_content_length_management() {
        let mut popup = NodeLogsPopup::new("test_node".to_string());

        // Start with no logs
        popup.set_logs(vec![]);
        assert_eq!(popup.logs.len(), 0);

        // Add some logs
        let test_logs: Vec<String> = (0..5).map(|i| format!("Log {i}")).collect();
        popup.set_logs(test_logs.clone());
        assert_eq!(popup.logs.len(), 5);
        assert_eq!(popup.logs, test_logs);

        // Add more logs dynamically
        popup.add_log_line("Extra log".to_string());
        assert_eq!(popup.logs.len(), 6);
        assert_eq!(popup.logs[5], "Extra log");
    }

    #[test]
    fn test_drawing_with_various_content_states() {
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();

        // Test drawing with no logs
        let mut popup_empty = NodeLogsPopup::new("empty_node".to_string());
        popup_empty.set_logs(vec![]);

        let result = terminal.draw(|f| {
            let area = f.area();
            if let Err(e) = popup_empty.draw(f, area) {
                panic!("Drawing failed with empty logs: {e}");
            }
        });
        assert!(result.is_ok());

        // Test drawing with many logs
        let mut popup_full = NodeLogsPopup::new("full_node".to_string());
        let many_logs: Vec<String> = (0..1000).map(|i| format!("Log line {i}")).collect();
        popup_full.set_logs(many_logs);

        let result = terminal.draw(|f| {
            let area = f.area();
            if let Err(e) = popup_full.draw(f, area) {
                panic!("Drawing failed with many logs: {e}");
            }
        });
        assert!(result.is_ok());

        // Test drawing with "No node available"
        let mut popup_no_node = NodeLogsPopup::new("No node available".to_string());

        let result = terminal.draw(|f| {
            let area = f.area();
            if let Err(e) = popup_no_node.draw(f, area) {
                panic!("Drawing failed with no node: {e}");
            }
        });
        assert!(result.is_ok());
    }

    #[test]
    fn test_tail_indicator_visibility() {
        let mut popup = NodeLogsPopup::new("test_node".to_string());
        let test_logs: Vec<String> = (0..10).map(|i| format!("Log line {i}")).collect();
        popup.set_logs(test_logs);

        // Should be following tail initially
        assert!(popup.is_following_tail);

        // Scroll up should disable tail following
        popup.handle_scroll_up();
        assert!(!popup.is_following_tail);

        // Back to bottom should re-enable
        popup.handle_end();
        assert!(popup.is_following_tail);
    }

    #[test]
    fn test_focus_target_consistency() {
        let popup1 = NodeLogsPopup::new("node1".to_string());
        let popup2 = NodeLogsPopup::new("node2".to_string());
        let popup3 = NodeLogsPopup::new("".to_string());

        // All instances should have the same focus target
        assert_eq!(popup1.focus_target(), FocusTarget::NodeLogsPopup);
        assert_eq!(popup2.focus_target(), FocusTarget::NodeLogsPopup);
        assert_eq!(popup3.focus_target(), FocusTarget::NodeLogsPopup);
    }

    #[test]
    fn test_log_content_with_special_characters() {
        let mut popup = NodeLogsPopup::new("test_node".to_string());
        let special_logs = vec![
            "Log with émojis 🚀 and ünïcødé".to_string(),
            "Log with\ttabs and\nnewlines".to_string(),
            "Log with very long line that exceeds normal width and should be handled gracefully by the display system".to_string(),
            "".to_string(), // Empty line
            "   Log with leading/trailing spaces   ".to_string(),
        ];

        popup.set_logs(special_logs.clone());
        assert_eq!(popup.logs, special_logs);

        // Should still function normally with special characters
        assert_eq!(popup.list_state.selected(), Some(4));
        popup.handle_home();
        assert_eq!(popup.list_state.selected(), Some(0));
    }
}
