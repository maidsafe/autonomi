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
    log_management::LogManagement,
    mode::Scene,
    style::{EUCALYPTUS, GHOST_WHITE, LIGHT_PERIWINKLE, RED, VERY_LIGHT_AZURE, clear_area},
    tui::Frame,
};
use arboard::Clipboard;
use chrono;
use color_eyre::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Style, Stylize},
    text::{Line, Span},
    widgets::{
        Block, Borders, List, ListItem, ListState, Padding, Paragraph, Scrollbar,
        ScrollbarOrientation, ScrollbarState, Wrap,
    },
};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc::UnboundedSender;
use tracing::{error, info};

#[derive(Debug, Clone, PartialEq)]
pub enum LogLoadingState {
    Loading,
    Loaded,
    Error(String),
}

#[derive(Debug, Clone, PartialEq)]
enum ScrollDirection {
    Up,
    Down,
    PageUp,
    PageDown,
    Home,
    End,
}

#[derive(Debug, Clone, Default)]
struct SelectionState {
    start: Option<usize>,
    end: Option<usize>,
    active: bool,
}

impl SelectionState {
    fn clear(&mut self) {
        self.start = None;
        self.end = None;
        self.active = false;
    }

    fn start_selection(&mut self, index: usize) {
        self.start = Some(index);
        self.end = Some(index);
        self.active = true;
    }

    fn extend_selection(&mut self, index: usize) {
        if self.start.is_none() {
            self.start_selection(index);
        } else {
            self.end = Some(index);
        }
    }

    fn select_all(&mut self, log_count: usize) {
        if log_count > 0 {
            self.start = Some(0);
            self.end = Some(log_count - 1);
            self.active = true;
        }
    }

    fn get_range(&self) -> Option<(usize, usize)> {
        match (self.start, self.end) {
            (Some(start), Some(end)) => {
                let min = start.min(end);
                let max = start.max(end);
                Some((min, max))
            }
            _ => None,
        }
    }

    fn is_line_selected(&self, index: usize) -> bool {
        if let Some((start, end)) = self.get_range() {
            index >= start && index <= end
        } else {
            false
        }
    }
}

#[derive(Debug, Clone)]
struct WordWrapState {
    scroll_offset: usize,
    cursor_offset: usize,
    window_size: usize,
}

impl Default for WordWrapState {
    fn default() -> Self {
        Self {
            scroll_offset: 0,
            cursor_offset: 0,
            window_size: 10,
        }
    }
}

#[derive(Debug, Clone)]
enum ViewMode {
    List {
        list_state: ListState,
        scroll_state: ScrollbarState,
    },
    WordWrap {
        state: WordWrapState,
    },
}

pub struct NodeLogsPopup {
    node_name: String,
    logs: Vec<String>,
    view_mode: ViewMode,
    selection: SelectionState,
    clipboard: Option<Clipboard>,
    log_dir: Option<PathBuf>,
    loading_state: LogLoadingState,
    total_lines: usize,
    log_management: Option<LogManagement>,
    action_sender: Option<UnboundedSender<Action>>,
    file_path: Option<String>,
    last_modified: Option<std::time::SystemTime>,
}

impl NodeLogsPopup {
    /// Format time difference in human-readable format
    fn format_time_ago(modified_time: SystemTime) -> String {
        let now = SystemTime::now();
        match now.duration_since(modified_time) {
            Ok(duration) => {
                let seconds = duration.as_secs();
                if seconds < 60 {
                    format!("{}s ago", seconds)
                } else if seconds < 3600 {
                    format!("{}min ago", seconds / 60)
                } else if seconds < 86400 {
                    format!("{} hours ago", seconds / 3600)
                } else {
                    // More than 24 hours, show exact time
                    if let Ok(duration_since_epoch) = modified_time.duration_since(UNIX_EPOCH) {
                        let timestamp = duration_since_epoch.as_secs();
                        // Simple timestamp format - could be enhanced with proper date formatting
                        chrono::DateTime::from_timestamp(timestamp as i64, 0)
                            .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
                            .unwrap_or_else(|| "Unknown time".to_string())
                    } else {
                        "Unknown time".to_string()
                    }
                }
            }
            Err(_) => "Future time".to_string(),
        }
    }

    /// Create a new NodeLogsPopup with LogManagement
    pub fn new(log_management: LogManagement) -> Self {
        Self {
            node_name: "".to_string(),
            logs: vec!["Initializing logs...".to_string()],
            view_mode: ViewMode::List {
                list_state: ListState::default(),
                scroll_state: ScrollbarState::default(),
            },
            selection: SelectionState::default(),
            clipboard: Clipboard::new().ok(),
            log_dir: None,
            loading_state: LogLoadingState::Loading,
            total_lines: 1,
            log_management: Some(log_management),
            action_sender: None,
            file_path: None,
            last_modified: None,
        }
    }

    /// Handle logs loaded from LogManagement system
    pub fn handle_logs_loaded(
        &mut self,
        logs: Vec<String>,
        total_lines: usize,
        file_path: Option<String>,
        last_modified: Option<std::time::SystemTime>,
    ) {
        self.loading_state = LogLoadingState::Loaded;
        self.logs = logs;
        self.total_lines = total_lines;
        self.file_path = file_path;
        self.last_modified = last_modified;

        if !self.logs.is_empty() {
            let last_index = self.logs.len() - 1;

            match &mut self.view_mode {
                ViewMode::List {
                    list_state,
                    scroll_state,
                } => {
                    list_state.select(Some(last_index));
                    *scroll_state = scroll_state
                        .position(last_index)
                        .content_length(self.logs.len());
                }
                ViewMode::WordWrap { state } => {
                    let max_scroll_offset = self.logs.len().saturating_sub(state.window_size);
                    state.scroll_offset = max_scroll_offset;
                    state.cursor_offset = 0;
                }
            }
        }
    }

    /// Handle log loading error from LogManagement system
    pub fn handle_logs_error(&mut self, error: String) {
        self.loading_state = LogLoadingState::Error(error.clone());
        self.logs = vec![
            "Error loading logs".to_string(),
            "".to_string(),
            error,
            "".to_string(),
            "You can try:".to_string(),
            "- Pressing 'R' to refresh logs".to_string(),
            "- Checking if the node is running".to_string(),
            "- Verifying the node name is correct".to_string(),
        ];

        if let ViewMode::List { scroll_state, .. } = &mut self.view_mode {
            *scroll_state = scroll_state.content_length(self.logs.len());
        }
    }

    pub fn set_node_name(
        &mut self,
        node_name: String,
        log_management: &LogManagement,
        action_sender: UnboundedSender<Action>,
    ) {
        if self.node_name == node_name {
            if self.logs.is_empty() {
                self.loading_state = LogLoadingState::Loading;
            } else {
                info!("Fetching logs for the same node again, do not change state");
            }
            self.loading_state = LogLoadingState::Loaded;
        }
        self.node_name = node_name.clone();

        // Request logs for the new node via LogManagement
        if let Err(e) = log_management.load_logs(node_name, self.log_dir.clone(), action_sender) {
            error!("Failed to send log loading task for new node: {e}");
            self.loading_state =
                LogLoadingState::Error(format!("Failed to start log loading: {e}"));
            self.logs = vec![format!("Error starting log loading: {e}")];
        }
    }

    pub fn add_log_line(&mut self, line: String) {
        self.logs.push(line);
    }

    pub fn set_logs(&mut self, logs: Vec<String>) {
        self.logs = logs;
        if let ViewMode::List { scroll_state, .. } = &mut self.view_mode {
            *scroll_state = scroll_state.content_length(self.logs.len());
        }
    }

    fn handle_scroll(&mut self, direction: ScrollDirection, with_shift: bool) {
        let (old_position, new_position) = match &mut self.view_mode {
            ViewMode::WordWrap { state } => {
                // Calculate old position before scrolling
                let old_pos = if !self.logs.is_empty() {
                    let pos = state.scroll_offset + state.cursor_offset;
                    Some(pos.min(self.logs.len().saturating_sub(1)))
                } else {
                    None
                };

                // Perform the scroll
                Self::handle_word_wrap_scroll_static(direction, state, &self.logs);

                // Calculate new position after scrolling
                let new_pos = if !self.logs.is_empty() {
                    let pos = state.scroll_offset + state.cursor_offset;
                    Some(pos.min(self.logs.len().saturating_sub(1)))
                } else {
                    None
                };

                (old_pos, new_pos)
            }
            ViewMode::List {
                list_state,
                scroll_state,
            } => {
                let old_pos = list_state.selected();
                let new_pos = Self::handle_list_scroll_static(
                    direction,
                    list_state,
                    scroll_state,
                    &self.logs,
                );
                (old_pos, new_pos)
            }
        };

        if with_shift {
            match (old_position, new_position) {
                (Some(old), Some(new)) => {
                    // Normal case: extending existing selection or starting new one
                    if !self.selection.active {
                        self.selection.start_selection(old);
                    }
                    self.selection.extend_selection(new);
                }
                (None, Some(new)) => {
                    // Starting selection from no current selection
                    self.selection.start_selection(new);
                }
                _ => {
                    // No valid position to work with, do nothing
                }
            }
        } else {
            self.selection.clear();
        }
    }

    fn handle_word_wrap_scroll_static(
        direction: ScrollDirection,
        state: &mut WordWrapState,
        logs: &[String],
    ) {
        match direction {
            ScrollDirection::Up => {
                if state.cursor_offset > 0 {
                    state.cursor_offset -= 1;
                } else if state.scroll_offset > 0 {
                    state.scroll_offset = state.scroll_offset.saturating_sub(1);
                }
            }
            ScrollDirection::Down => {
                let current_position = state.scroll_offset + state.cursor_offset;
                if current_position < logs.len().saturating_sub(1) {
                    let window_size = state.window_size;
                    let start = state.scroll_offset.min(logs.len().saturating_sub(1));
                    let end = (start + window_size).min(logs.len());
                    let actual_lines_displayed = end - start;
                    let max_cursor_in_displayed = actual_lines_displayed.saturating_sub(1);

                    if state.cursor_offset < max_cursor_in_displayed {
                        state.cursor_offset += 1;
                    } else {
                        let max_scroll_offset = logs.len().saturating_sub(window_size);
                        if state.scroll_offset < max_scroll_offset {
                            state.scroll_offset += 1;
                            let new_start = state.scroll_offset.min(logs.len().saturating_sub(1));
                            let new_end = (new_start + window_size).min(logs.len());
                            let new_actual_lines = new_end - new_start;
                            if state.cursor_offset >= new_actual_lines {
                                state.cursor_offset = new_actual_lines.saturating_sub(1);
                            }
                        }
                    }
                }
            }
            ScrollDirection::PageUp => {
                let page_size = 10;
                if state.scroll_offset >= page_size {
                    state.scroll_offset -= page_size;
                } else {
                    state.scroll_offset = 0;
                    state.cursor_offset = 0;
                }
            }
            ScrollDirection::PageDown => {
                let page_size = 10;
                let max_scroll_offset = logs.len().saturating_sub(state.window_size);
                let new_offset = (state.scroll_offset + page_size).min(max_scroll_offset);
                state.scroll_offset = new_offset;
                state.cursor_offset = 0;
            }
            ScrollDirection::Home => {
                state.scroll_offset = 0;
                state.cursor_offset = 0;
            }
            ScrollDirection::End => {
                let max_scroll_offset = logs.len().saturating_sub(state.window_size);
                state.scroll_offset = max_scroll_offset;
                state.cursor_offset = 0;
            }
        }
    }

    fn handle_list_scroll_static(
        direction: ScrollDirection,
        list_state: &mut ListState,
        scroll_state: &mut ScrollbarState,
        logs: &[String],
    ) -> Option<usize> {
        let selected = list_state.selected();

        match direction {
            ScrollDirection::Up => {
                if let Some(current) = selected {
                    if current > 0 {
                        let new_pos = current - 1;
                        list_state.select(Some(new_pos));
                        *scroll_state = scroll_state.position(new_pos);
                        Some(new_pos)
                    } else {
                        None
                    }
                } else if !logs.is_empty() {
                    let last_index = logs.len() - 1;
                    list_state.select(Some(last_index));
                    Some(last_index)
                } else {
                    None
                }
            }
            ScrollDirection::Down => {
                if let Some(current) = selected {
                    if current < logs.len().saturating_sub(1) {
                        let new_pos = current + 1;
                        list_state.select(Some(new_pos));
                        *scroll_state = scroll_state.position(new_pos);
                        Some(new_pos)
                    } else {
                        None
                    }
                } else if !logs.is_empty() {
                    list_state.select(Some(0));
                    *scroll_state = scroll_state.position(0);
                    Some(0)
                } else {
                    None
                }
            }
            ScrollDirection::PageUp => {
                if let Some(current) = selected {
                    let new_pos = current.saturating_sub(10);
                    list_state.select(Some(new_pos));
                    *scroll_state = scroll_state.position(new_pos);
                    Some(new_pos)
                } else {
                    None
                }
            }
            ScrollDirection::PageDown => {
                if let Some(current) = selected {
                    let new_pos = (current + 10).min(logs.len().saturating_sub(1));
                    list_state.select(Some(new_pos));
                    *scroll_state = scroll_state.position(new_pos);
                    Some(new_pos)
                } else {
                    None
                }
            }
            ScrollDirection::Home => {
                if !logs.is_empty() {
                    list_state.select(Some(0));
                    *scroll_state = scroll_state.position(0);
                    Some(0)
                } else {
                    None
                }
            }
            ScrollDirection::End => {
                if !logs.is_empty() {
                    let last_index = logs.len() - 1;
                    list_state.select(Some(last_index));
                    *scroll_state = scroll_state.position(last_index);
                    Some(last_index)
                } else {
                    None
                }
            }
        }
    }

    fn get_selected_text(&self) -> String {
        if let Some((start, end)) = self.selection.get_range() {
            self.logs[start..=end].join("\n")
        } else {
            match &self.view_mode {
                ViewMode::List { list_state, .. } => {
                    if let Some(current) = list_state.selected() {
                        self.logs.get(current).cloned().unwrap_or_default()
                    } else {
                        String::new()
                    }
                }
                ViewMode::WordWrap { state } => {
                    let current_position = state.scroll_offset + state.cursor_offset;
                    self.logs.get(current_position).cloned().unwrap_or_default()
                }
            }
        }
    }

    fn copy_to_clipboard(&mut self) -> Result<()> {
        let text = self.get_selected_text();
        if !text.is_empty()
            && let Some(ref mut clipboard) = self.clipboard
        {
            clipboard.set_text(text)?;
        }
        Ok(())
    }

    fn toggle_word_wrap(&mut self) {
        match &mut self.view_mode {
            ViewMode::List {
                list_state,
                scroll_state: _,
            } => {
                let selected = list_state.selected().unwrap_or(0);
                let mut wrap_state = WordWrapState::default();
                wrap_state.scroll_offset = selected.saturating_sub(5);
                wrap_state.cursor_offset = selected.saturating_sub(wrap_state.scroll_offset).min(5);
                self.view_mode = ViewMode::WordWrap { state: wrap_state };
            }
            ViewMode::WordWrap { state } => {
                let absolute_position = state.scroll_offset + state.cursor_offset;
                let clamped_position = absolute_position.min(self.logs.len().saturating_sub(1));
                let mut list_state = ListState::default();
                list_state.select(Some(clamped_position));
                let scroll_state = ScrollbarState::default()
                    .position(clamped_position)
                    .content_length(self.logs.len());
                self.view_mode = ViewMode::List {
                    list_state,
                    scroll_state,
                };
            }
        }
    }

    fn render_title_with_file_info(&self, f: &mut Frame<'_>, popup_area: Rect) {
        let (title, title_style) = match &self.loading_state {
            LogLoadingState::Loading => (
                format!(" Node Logs - {} [LOADING...] ", self.node_name),
                Style::default().fg(LIGHT_PERIWINKLE).bold(),
            ),
            LogLoadingState::Loaded => (
                format!(" Node Logs - {} ", self.node_name),
                Style::default().fg(EUCALYPTUS).bold(),
            ),
            LogLoadingState::Error(_) => (
                format!(" Node Logs - {} [ERROR] ", self.node_name),
                Style::default().fg(RED).bold(),
            ),
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .title(title)
            .title_style(title_style)
            .border_style(Style::default().fg(EUCALYPTUS));

        f.render_widget(block, popup_area);

        // Render file info on the first line inside the border
        if matches!(self.loading_state, LogLoadingState::Loaded) {
            let mut line_parts = Vec::new();

            if let Some(ref file_path) = self.file_path {
                let display_path = format!("../{file_path}");
                line_parts.push(Span::styled(display_path, Style::default().fg(GHOST_WHITE)));

                if let Some(last_modified) = self.last_modified {
                    let time_str = Self::format_time_ago(last_modified);
                    line_parts.push(Span::styled(
                        " (modified: ",
                        Style::default().fg(LIGHT_PERIWINKLE),
                    ));
                    line_parts.push(Span::styled(time_str, Style::default().fg(GHOST_WHITE)));
                    line_parts.push(Span::styled(")", Style::default().fg(LIGHT_PERIWINKLE)));
                }
            }

            if !line_parts.is_empty() {
                let file_info_area = Rect {
                    x: popup_area.x + 2,
                    y: popup_area.y + 2,
                    width: popup_area.width.saturating_sub(4),
                    height: 1,
                };

                let file_info_line = Line::from(line_parts);
                let file_info_paragraph =
                    Paragraph::new(vec![file_info_line]).style(Style::default().fg(GHOST_WHITE));
                f.render_widget(file_info_paragraph, file_info_area);
            }
        }
    }

    fn render_loading(&self, f: &mut Frame<'_>, area: Rect) {
        let loading_text = vec![
            Line::from(Span::styled(
                "Loading logs, please wait...",
                Style::default().fg(LIGHT_PERIWINKLE).bold(),
            )),
            Line::from(""),
            Line::from(Span::styled("⣾ Loading", Style::default().fg(EUCALYPTUS))),
        ];
        let loading_paragraph = Paragraph::new(loading_text)
            .alignment(Alignment::Center)
            .style(Style::default().fg(GHOST_WHITE));
        f.render_widget(loading_paragraph, area);
    }

    fn render_logs_content(&mut self, f: &mut Frame<'_>, logs_area: [Rect; 2]) {
        match &mut self.view_mode {
            ViewMode::WordWrap { state } => {
                Self::render_word_wrap_view_static(f, logs_area[0], state, &self.logs);
            }
            ViewMode::List {
                list_state,
                scroll_state,
            } => {
                Self::render_list_view_static(
                    f,
                    logs_area,
                    list_state,
                    scroll_state,
                    &self.logs,
                    &self.selection,
                );
            }
        }
    }

    fn render_word_wrap_view_static(
        f: &mut Frame<'_>,
        area: Rect,
        state: &mut WordWrapState,
        logs: &[String],
    ) {
        let viewport_height = area.height as usize;
        let window_size = (viewport_height / 2).max(10);
        state.window_size = window_size;

        let start = state.scroll_offset.min(logs.len().saturating_sub(1));
        let end = (start + window_size).min(logs.len());
        let display_logs = &logs[start..end];

        let actual_lines_displayed = display_logs.len();
        let display_cursor = state
            .cursor_offset
            .min(actual_lines_displayed.saturating_sub(1));

        let text_lines: Vec<Line> = display_logs
            .iter()
            .enumerate()
            .map(|(i, line)| {
                let style = if i == display_cursor {
                    Style::default().fg(GHOST_WHITE).bg(VERY_LIGHT_AZURE)
                } else {
                    Style::default().fg(GHOST_WHITE)
                };
                Line::from(Span::styled(line.clone(), style))
            })
            .collect();

        let paragraph = Paragraph::new(text_lines)
            .wrap(Wrap { trim: true })
            .style(Style::default().fg(GHOST_WHITE));

        f.render_widget(paragraph, area);
    }

    fn render_list_view_static(
        f: &mut Frame<'_>,
        logs_area: [Rect; 2],
        list_state: &mut ListState,
        scroll_state: &mut ScrollbarState,
        logs: &[String],
        selection: &SelectionState,
    ) {
        let log_items: Vec<ListItem> = logs
            .iter()
            .enumerate()
            .map(|(i, log)| {
                let style = if Some(i) == list_state.selected() {
                    Style::default().fg(GHOST_WHITE).bg(VERY_LIGHT_AZURE)
                } else if selection.is_line_selected(i) {
                    Style::default().fg(GHOST_WHITE).bg(LIGHT_PERIWINKLE)
                } else {
                    Style::default().fg(GHOST_WHITE)
                };
                ListItem::new(log.clone()).style(style)
            })
            .collect();

        let logs_list = List::new(log_items)
            .style(Style::default().fg(GHOST_WHITE))
            .highlight_style(Style::default().fg(GHOST_WHITE).bg(VERY_LIGHT_AZURE));

        f.render_stateful_widget(logs_list, logs_area[0], list_state);

        let scrollbar = Scrollbar::default()
            .orientation(ScrollbarOrientation::VerticalRight)
            .style(Style::default().fg(LIGHT_PERIWINKLE));

        f.render_stateful_widget(scrollbar, logs_area[1], scroll_state);
    }

    fn render_instructions(&self, f: &mut Frame<'_>, area: Rect) {
        let selection_count = if let Some((start, end)) = self.selection.get_range() {
            format!(" {} lines selected", end - start + 1)
        } else {
            String::new()
        };

        let instructions = Paragraph::new(vec![
            Line::from(vec![
                Span::styled("↑/↓", Style::default().fg(EUCALYPTUS).bold()),
                Span::styled(" Scroll  ", Style::default().fg(GHOST_WHITE)),
                Span::styled("Shift+↑/↓", Style::default().fg(EUCALYPTUS).bold()),
                Span::styled(" Select  ", Style::default().fg(GHOST_WHITE)),
                Span::styled("Ctrl+C", Style::default().fg(EUCALYPTUS).bold()),
                Span::styled(" Copy", Style::default().fg(GHOST_WHITE)),
            ]),
            Line::from(vec![
                Span::styled("ESC", Style::default().fg(RED).bold()),
                Span::styled(" Close  ", Style::default().fg(GHOST_WHITE)),
                Span::styled("W", Style::default().fg(EUCALYPTUS).bold()),
                Span::styled(" Word Wrap  ", Style::default().fg(GHOST_WHITE)),
                Span::styled("Ctrl+A", Style::default().fg(EUCALYPTUS).bold()),
                Span::styled(" Select All", Style::default().fg(GHOST_WHITE)),
                Span::styled(&selection_count, Style::default().fg(LIGHT_PERIWINKLE)),
            ]),
        ])
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .borders(Borders::TOP)
                .border_style(Style::default().fg(EUCALYPTUS))
                .padding(Padding::uniform(1)),
        );

        f.render_widget(instructions, area);
    }

    fn render_word_wrap_indicator(&self, f: &mut Frame<'_>, popup_area: Rect) {
        if matches!(self.view_mode, ViewMode::WordWrap { .. }) {
            let indicator_text = " [WRAP] ";
            let indicator_width = indicator_text.len() as u16;
            let indicator_area = Rect::new(
                popup_area.right().saturating_sub(indicator_width + 1),
                popup_area.y + 1,
                indicator_width,
                1,
            );
            let indicator =
                Paragraph::new(indicator_text).style(Style::default().fg(EUCALYPTUS).bold());
            f.render_widget(indicator, indicator_area);
        }
    }
}

impl Component for NodeLogsPopup {
    fn focus_target(&self) -> FocusTarget {
        FocusTarget::NodeLogsPopup
    }

    fn register_action_handler(&mut self, tx: UnboundedSender<Action>) -> Result<()> {
        self.action_sender = Some(tx);
        Ok(())
    }

    fn handle_key_events(
        &mut self,
        key: KeyEvent,
        _focus_manager: &FocusManager,
    ) -> Result<(Vec<Action>, EventResult)> {
        let shift_pressed = key.modifiers.contains(KeyModifiers::SHIFT);
        let ctrl_pressed = key.modifiers.contains(KeyModifiers::CONTROL);

        let action = match key.code {
            KeyCode::Esc => Action::SwitchScene(Scene::Status),
            KeyCode::Up => {
                self.handle_scroll(ScrollDirection::Up, shift_pressed);
                return Ok((vec![], EventResult::Consumed));
            }
            KeyCode::Down => {
                self.handle_scroll(ScrollDirection::Down, shift_pressed);
                return Ok((vec![], EventResult::Consumed));
            }
            KeyCode::PageUp => {
                self.handle_scroll(ScrollDirection::PageUp, shift_pressed);
                return Ok((vec![], EventResult::Consumed));
            }
            KeyCode::PageDown => {
                self.handle_scroll(ScrollDirection::PageDown, shift_pressed);
                return Ok((vec![], EventResult::Consumed));
            }
            KeyCode::Home => {
                self.handle_scroll(ScrollDirection::Home, shift_pressed);
                return Ok((vec![], EventResult::Consumed));
            }
            KeyCode::End => {
                self.handle_scroll(ScrollDirection::End, shift_pressed);
                return Ok((vec![], EventResult::Consumed));
            }
            KeyCode::Char('c') if ctrl_pressed => {
                if let Err(e) = self.copy_to_clipboard() {
                    error!("Failed to copy to clipboard: {e}");
                }
                return Ok((vec![], EventResult::Consumed));
            }
            KeyCode::Char('a') if ctrl_pressed => {
                self.selection.select_all(self.logs.len());
                return Ok((vec![], EventResult::Consumed));
            }
            KeyCode::Char('w') | KeyCode::Char('W') => {
                self.toggle_word_wrap();
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
                if let (Some(log_management), Some(action_sender)) =
                    (self.log_management.clone(), self.action_sender.clone())
                {
                    self.set_node_name(node_name, &log_management, action_sender);
                } else {
                    error!("LogManagement or action_sender not available for SetNodeLogsTarget");
                }
                Ok(Some(Action::SwitchScene(Scene::NodeLogsPopUp)))
            }
            Action::LogsLoaded {
                node_name,
                logs,
                total_lines,
                file_path,
                last_modified,
            } => {
                info!("Logs loaded for node: {node_name}, total lines: {total_lines}");
                if node_name == self.node_name {
                    self.handle_logs_loaded(logs, total_lines, file_path, last_modified);
                }
                Ok(None)
            }
            Action::LogsLoadError { node_name, error } => {
                error!("Failed to load logs for node: {node_name}, error: {error}");
                if node_name == self.node_name {
                    self.handle_logs_error(error);
                }
                Ok(None)
            }
            _ => Ok(None),
        }
    }

    fn draw(&mut self, f: &mut Frame<'_>, area: Rect) -> Result<()> {
        let popup_area = centered_rect(80, 85, area);
        clear_area(f, popup_area);

        let main_layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(1),    // Logs
                Constraint::Length(5), // Instructions (2 lines + padding + border)
            ])
            .split(popup_area);

        self.render_title_with_file_info(f, popup_area);

        // Create a custom logs area that starts after the file info line with 1 line gap
        let logs_content_area = Rect {
            x: popup_area.x + 1,
            y: popup_area.y + 4, // title border (1) + file info line (2) + gap (3) + start logs (4)
            width: popup_area.width.saturating_sub(2),
            height: main_layout[0].height.saturating_sub(3), // subtract title + file info + gap
        };

        let logs_area = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(1), Constraint::Length(2)])
            .split(logs_content_area);

        if matches!(self.loading_state, LogLoadingState::Loading) {
            self.render_loading(f, logs_area[0]);
        } else {
            self.render_logs_content(f, [logs_area[0], logs_area[1]]);
        }

        self.render_instructions(f, main_layout[1]);
        self.render_word_wrap_indicator(f, popup_area);

        Ok(())
    }
}
