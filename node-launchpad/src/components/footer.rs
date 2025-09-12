// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::{
    components::node_table::NodeDisplayStatus,
    style::{COOL_GREY, EUCALYPTUS, GHOST_WHITE, LIGHT_PERIWINKLE},
};
use ratatui::{prelude::*, widgets::*};

#[derive(Debug, Clone)]
pub struct FooterState {
    pub has_nodes: bool,
    pub has_running_nodes: bool,
    pub selected_node_status: Option<NodeDisplayStatus>,
    pub rewards_address_set: bool,
}

#[derive(Default)]
pub struct Footer {}

impl Footer {
    /// Get command bracket style - always white when enabled, grey when disabled
    fn command_style(enabled: bool) -> Style {
        if enabled {
            Style::default().fg(GHOST_WHITE)
        } else {
            Style::default().fg(LIGHT_PERIWINKLE)
        }
    }

    /// Get disabled text style
    fn disabled_text_style() -> Style {
        Style::default().fg(COOL_GREY)
    }

    /// Add command - always green when rewards address is set
    fn add_styles(state: &FooterState) -> (Style, Style) {
        let enabled = state.rewards_address_set;
        (
            Self::command_style(enabled),
            if enabled {
                Style::default().fg(EUCALYPTUS)
            } else {
                Self::disabled_text_style()
            },
        )
    }

    /// Remove command - enabled when a node is selected
    fn remove_styles(state: &FooterState) -> (Style, Style) {
        let enabled = state.selected_node_status.is_some();
        (
            Self::command_style(enabled),
            if enabled {
                Style::default().fg(LIGHT_PERIWINKLE)
            } else {
                Self::disabled_text_style()
            },
        )
    }

    /// Toggle command - enabled when a node is selected, style depends on selected node status
    fn toggle_styles(state: &FooterState) -> (Style, Style) {
        match &state.selected_node_status {
            Some(NodeDisplayStatus::Running) => (
                Self::command_style(true),
                Style::default().fg(GHOST_WHITE), // White for stopping
            ),
            Some(NodeDisplayStatus::Stopped | NodeDisplayStatus::Added) => (
                Self::command_style(true),
                Style::default().fg(EUCALYPTUS), // Green for starting
            ),
            _ => (Self::command_style(false), Self::disabled_text_style()),
        }
    }

    /// Open Logs command - enabled when a node is selected
    fn logs_styles(state: &FooterState) -> (Style, Style) {
        let enabled = state.selected_node_status.is_some();
        (
            Self::command_style(enabled),
            if enabled {
                Style::default().fg(LIGHT_PERIWINKLE)
            } else {
                Self::disabled_text_style()
            },
        )
    }

    /// Manage command - enabled when nodes exist and rewards address is set
    fn manage_styles(state: &FooterState) -> (Style, Style) {
        let enabled = state.has_nodes && state.rewards_address_set;
        (
            Self::command_style(enabled),
            if enabled {
                Style::default().fg(EUCALYPTUS)
            } else {
                Self::disabled_text_style()
            },
        )
    }

    /// Run All command - enabled when nodes exist but not all are running
    fn run_all_styles(state: &FooterState) -> (Style, Style) {
        // Enable if we have nodes and either no nodes are running OR there are some nodes not running
        let enabled = state.has_nodes && !state.has_running_nodes;
        (
            Self::command_style(enabled),
            if enabled {
                Style::default().fg(EUCALYPTUS)
            } else {
                Self::disabled_text_style()
            },
        )
    }

    /// Stop All command - enabled when there are running nodes
    fn stop_all_styles(state: &FooterState) -> (Style, Style) {
        let enabled = state.has_running_nodes;
        (
            Self::command_style(enabled),
            if enabled {
                Style::default().fg(LIGHT_PERIWINKLE)
            } else {
                Self::disabled_text_style()
            },
        )
    }
}

impl StatefulWidget for Footer {
    type State = FooterState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints(vec![Constraint::Length(3)])
            .split(area);

        // Get styles for each command
        let (add_cmd_style, add_text_style) = Footer::add_styles(state);
        let (remove_cmd_style, remove_text_style) = Footer::remove_styles(state);
        let (toggle_cmd_style, toggle_text_style) = Footer::toggle_styles(state);
        let (logs_cmd_style, logs_text_style) = Footer::logs_styles(state);
        let (manage_cmd_style, manage_text_style) = Footer::manage_styles(state);
        let (run_all_cmd_style, run_all_text_style) = Footer::run_all_styles(state);
        let (stop_all_cmd_style, stop_all_text_style) = Footer::stop_all_styles(state);

        let commands = vec![
            Span::styled("[+] ", add_cmd_style),
            Span::styled("Add", add_text_style),
            Span::styled(" ", Style::default()),
            Span::styled("[-] ", remove_cmd_style),
            Span::styled("Remove", remove_text_style),
            Span::styled(" ", Style::default()),
            Span::styled("[Ctrl+S] ", toggle_cmd_style),
            Span::styled("Toggle Node", toggle_text_style),
            Span::styled(" ", Style::default()),
            Span::styled("[L] ", logs_cmd_style),
            Span::styled("Open Logs", logs_text_style),
        ];

        let stop_all = vec![
            Span::styled("[Ctrl+G] ", manage_cmd_style),
            Span::styled("Manage", manage_text_style),
            Span::styled(" ", Style::default()),
            Span::styled("[Ctrl+R] ", run_all_cmd_style),
            Span::styled("Run All", run_all_text_style),
            Span::styled(" ", Style::default()),
            Span::styled("[Ctrl+X] ", stop_all_cmd_style),
            Span::styled("Stop All", stop_all_text_style),
        ];

        let total_width = (layout[0].width - 1) as usize;
        let spaces = " ".repeat(total_width.saturating_sub(
            commands.iter().map(|s| s.width()).sum::<usize>()
                + stop_all.iter().map(|s| s.width()).sum::<usize>(),
        ));

        let commands_length = 6 + commands.iter().map(|s| s.width()).sum::<usize>() as u16;
        let spaces_length = spaces.len().saturating_sub(6) as u16;
        let stop_all_length = stop_all.iter().map(|s| s.width()).sum::<usize>() as u16;

        let cell1 = Cell::from(Line::from(commands));
        let cell2 = Cell::from(Line::raw(spaces));
        let cell3 = Cell::from(Line::from(stop_all));
        let row = Row::new(vec![cell1, cell2, cell3]);

        let table = Table::new(
            [row],
            [
                Constraint::Length(commands_length),
                Constraint::Length(spaces_length),
                Constraint::Length(stop_all_length),
            ],
        )
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(EUCALYPTUS))
                .padding(Padding::horizontal(1)),
        );

        StatefulWidget::render(table, area, buf, &mut TableState::default());
    }
}
