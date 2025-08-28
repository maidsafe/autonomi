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
    config::Config,
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
                self.list_state.select(Some(selected + 1));
                self.scroll_state = self.scroll_state.position(selected + 1);
            } else {
                // At the bottom, enable tail following
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
    fn init(&mut self, _area: Rect) -> Result<()> {
        Ok(())
    }

    fn register_action_handler(
        &mut self,
        _action_sender: tokio::sync::mpsc::UnboundedSender<Action>,
    ) -> Result<()> {
        Ok(())
    }

    fn register_config_handler(&mut self, _config: Config) -> Result<()> {
        Ok(())
    }

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
