// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

pub const NODE_WIDTH: usize = 10;
pub const VERSION_WIDTH: usize = 7;
pub const ATTOS_WIDTH: usize = 5;
pub const MEMORY_WIDTH: usize = 7;
pub const MBPS_WIDTH: usize = 13;
pub const RECORDS_WIDTH: usize = 4;
pub const PEERS_WIDTH: usize = 5;
pub const CONNS_WIDTH: usize = 5;
pub const MODE_WIDTH: usize = 7;
pub const STATUS_WIDTH: usize = 8;
pub const FAILURE_WIDTH: usize = 64;
pub const SPINNER_WIDTH: usize = 1;

use super::state::NodeTableState;
use crate::style::{COOL_GREY, EUCALYPTUS, GHOST_WHITE, INDIGO, LIGHT_PERIWINKLE};
use ratatui::{prelude::*, widgets::*};
use throbber_widgets_tui::{Throbber, WhichUse};

// Re-export config from state module for convenience
pub use super::state::NodeTableConfig;

pub struct NodeTableWidget;

impl NodeTableWidget {
    pub fn render(self, area: Rect, f: &mut crate::tui::Frame<'_>, state: &mut NodeTableState) {
        // Render the node table
        let block_nodes = Block::default()
            .title(Line::from(vec![
                Span::styled(" Nodes", Style::default().fg(GHOST_WHITE).bold()),
                Span::styled(
                    format!(" ({}) ", state.items.items.len()),
                    Style::default().fg(LIGHT_PERIWINKLE),
                ),
            ]))
            .padding(Padding::new(1, 1, 0, 0))
            .title_style(Style::default().fg(GHOST_WHITE))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(EUCALYPTUS));

        let inner_area = block_nodes.inner(area);

        let node_widths = [
            Constraint::Min(NODE_WIDTH as u16),
            Constraint::Min(VERSION_WIDTH as u16),
            Constraint::Min(ATTOS_WIDTH as u16),
            Constraint::Min(MEMORY_WIDTH as u16),
            Constraint::Min(MBPS_WIDTH as u16),
            Constraint::Min(RECORDS_WIDTH as u16),
            Constraint::Min(PEERS_WIDTH as u16),
            Constraint::Min(CONNS_WIDTH as u16),
            Constraint::Min(MODE_WIDTH as u16),
            Constraint::Min(STATUS_WIDTH as u16),
            Constraint::Fill(FAILURE_WIDTH as u16),
            Constraint::Max(SPINNER_WIDTH as u16),
        ];

        let header_row = Row::new(vec![
            Cell::new("Node").fg(COOL_GREY),
            Cell::new("Version").fg(COOL_GREY),
            Cell::new("Attos").fg(COOL_GREY),
            Cell::new("Memory").fg(COOL_GREY),
            Cell::new(format!(
                "{}{}",
                " ".repeat(MBPS_WIDTH - "Mbps".len()),
                "Mbps"
            ))
            .fg(COOL_GREY),
            Cell::new("Recs").fg(COOL_GREY),
            Cell::new("Peers").fg(COOL_GREY),
            Cell::new("Conns").fg(COOL_GREY),
            Cell::new("Mode").fg(COOL_GREY),
            Cell::new("Status").fg(COOL_GREY),
            Cell::new("Failure").fg(COOL_GREY),
            Cell::new(" ").fg(COOL_GREY),
        ])
        .style(Style::default().add_modifier(Modifier::BOLD));

        let mut table_rows: Vec<Row> = Vec::new();

        for (i, node_item) in state.items.items.iter().enumerate() {
            let is_selected = state.items.state.selected() == Some(i);

            let failure = node_item.failure.as_ref().map_or_else(
                || "-".to_string(),
                |(_dt, msg)| {
                    if node_item.status == super::node_item::NodeStatus::Stopped {
                        msg.clone()
                    } else {
                        "-".to_string()
                    }
                },
            );

            let row_style = if is_selected {
                Style::default().fg(GHOST_WHITE).bg(INDIGO)
            } else {
                Style::default().fg(GHOST_WHITE)
            };

            let row_data = vec![
                node_item.name.clone(),
                node_item.version.clone(),
                format!("{:>width$}", node_item.attos, width = ATTOS_WIDTH),
                format!(
                    "{:>width$} MB",
                    node_item.memory,
                    width = MEMORY_WIDTH.saturating_sub(3)
                ),
                format!("{:>width$}", node_item.mbps, width = MBPS_WIDTH),
                format!("{:>width$}", node_item.records, width = RECORDS_WIDTH),
                format!("{:>width$}", node_item.peers, width = PEERS_WIDTH),
                format!("{:>width$}", node_item.connections, width = CONNS_WIDTH),
                node_item.mode.to_string(),
                node_item.status.to_string(),
                failure,
            ];

            table_rows.push(Row::new(row_data).style(row_style));
        }

        let table = Table::new(table_rows, node_widths)
            .header(header_row)
            .column_spacing(1)
            .row_highlight_style(Style::default().bg(INDIGO))
            .highlight_spacing(HighlightSpacing::Always);

        f.render_stateful_widget(table, inner_area, &mut state.items.state);
        f.render_widget(block_nodes, area);

        // Render spinners for each row
        self.render_spinners(inner_area, f, state);

        // Render error popup if visible
        if let Some(error_popup) = &state.error_popup
            && error_popup.is_visible()
        {
            // For now, we'll just note that error popup rendering would happen here
            // In the full implementation, we'd need to handle this properly
        }
    }

    fn render_spinners(
        self,
        table_area: Rect,
        f: &mut crate::tui::Frame<'_>,
        state: &mut NodeTableState,
    ) {
        use super::node_item::NodeStatus;

        // Calculate the spinner column position (rightmost column)
        let spinner_x = table_area.right().saturating_sub(2);

        // Start after the header row (y + 1)
        let start_y = table_area.y + 1;

        for (i, node_item) in state.items.items.iter().enumerate() {
            if i >= state.spinner_states.len() {
                break;
            }

            let spinner_area = Rect::new(spinner_x, start_y + i as u16, 1, 1);

            let spinner = match node_item.status {
                NodeStatus::Running => Throbber::default()
                    .throbber_style(Style::default().fg(EUCALYPTUS).add_modifier(Modifier::BOLD))
                    .throbber_set(throbber_widgets_tui::BRAILLE_SIX_DOUBLE)
                    .use_type(WhichUse::Spin),
                NodeStatus::Starting => Throbber::default()
                    .throbber_style(Style::default().fg(EUCALYPTUS).add_modifier(Modifier::BOLD))
                    .throbber_set(throbber_widgets_tui::BOX_DRAWING)
                    .use_type(WhichUse::Spin),
                NodeStatus::Stopped => Throbber::default()
                    .throbber_style(
                        Style::default()
                            .fg(GHOST_WHITE)
                            .add_modifier(Modifier::BOLD),
                    )
                    .throbber_set(throbber_widgets_tui::BRAILLE_SIX_DOUBLE)
                    .use_type(WhichUse::Full),
                NodeStatus::Updating => Throbber::default()
                    .throbber_style(
                        Style::default()
                            .fg(GHOST_WHITE)
                            .add_modifier(Modifier::BOLD),
                    )
                    .throbber_set(throbber_widgets_tui::VERTICAL_BLOCK)
                    .use_type(WhichUse::Spin),
                _ => Throbber::default()
                    .throbber_style(Style::default().fg(GHOST_WHITE))
                    .use_type(WhichUse::Full),
            };

            f.render_stateful_widget(spinner, spinner_area, &mut state.spinner_states[i]);
        }
    }
}
