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
pub const STATUS_WIDTH: usize = 24;
pub const SPINNER_WIDTH: usize = 1;
const NUMBER_OF_COLUMNS: usize = 10;
const STARTUP_CHECK_LABEL: &str = "Startup check";
const MIN_BAR_INNER: usize = 3;
const MAX_BAR_INNER: usize = 10;
const MIN_BAR_WIDTH: usize = MIN_BAR_INNER + 2;
const MAX_BAR_WIDTH: usize = MAX_BAR_INNER + 2;
const MAX_PERCENT_WIDTH: usize = 4;
const STATUS_MAX_WIDTH: usize =
    STARTUP_CHECK_LABEL.len() + 1 + MAX_BAR_WIDTH + 1 + MAX_PERCENT_WIDTH;

use super::{
    node_item::{NodeDisplayStatus, NodeItem},
    state::NodeTableState,
};
use crate::style::{COOL_GREY, DARK_GUNMETAL, EUCALYPTUS, GHOST_WHITE, INDIGO, LIGHT_PERIWINKLE};
use ant_service_management::{ReachabilityProgress, metric::ReachabilityStatusValues};
use ratatui::{prelude::*, widgets::*};
use throbber_widgets_tui::{Throbber, ThrobberState, WhichUse};

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

        let reachability_active = state.items.items.iter().any(|node| {
            matches!(
                node.node_display_status,
                NodeDisplayStatus::ReachabilityCheck
            )
        });

        let (node_widths, status_column_width) = if reachability_active {
            let fixed_columns: u16 = NODE_WIDTH as u16
                + VERSION_WIDTH as u16
                + ATTOS_WIDTH as u16
                + MEMORY_WIDTH as u16
                + MBPS_WIDTH as u16
                + RECORDS_WIDTH as u16
                + PEERS_WIDTH as u16
                + CONNS_WIDTH as u16
                + SPINNER_WIDTH as u16;
            let column_spacing = 1u16;
            let total_spacing = column_spacing * (NUMBER_OF_COLUMNS as u16 - 1);
            let available_for_status = inner_area
                .width
                .saturating_sub(fixed_columns + total_spacing);
            let status_column_width = available_for_status
                .min(STATUS_MAX_WIDTH as u16)
                .max(STATUS_WIDTH as u16) as usize;

            (
                [
                    Constraint::Min(NODE_WIDTH as u16),
                    Constraint::Min(VERSION_WIDTH as u16),
                    Constraint::Min(ATTOS_WIDTH as u16),
                    Constraint::Min(MEMORY_WIDTH as u16),
                    Constraint::Min(MBPS_WIDTH as u16),
                    Constraint::Min(RECORDS_WIDTH as u16),
                    Constraint::Min(PEERS_WIDTH as u16),
                    Constraint::Min(CONNS_WIDTH as u16),
                    Constraint::Min(status_column_width as u16),
                    Constraint::Length(SPINNER_WIDTH as u16),
                ],
                status_column_width,
            )
        } else {
            (
                [
                    Constraint::Min(NODE_WIDTH as u16),
                    Constraint::Min(VERSION_WIDTH as u16),
                    Constraint::Min(ATTOS_WIDTH as u16),
                    Constraint::Min(MEMORY_WIDTH as u16),
                    Constraint::Min(MBPS_WIDTH as u16),
                    Constraint::Min(RECORDS_WIDTH as u16),
                    Constraint::Min(PEERS_WIDTH as u16),
                    Constraint::Min(CONNS_WIDTH as u16),
                    Constraint::Min(STATUS_WIDTH as u16),
                    Constraint::Length(SPINNER_WIDTH as u16),
                ],
                STATUS_WIDTH,
            )
        };

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
            Cell::new("Status").fg(COOL_GREY),
            Cell::new(" ").fg(COOL_GREY),
        ])
        .style(Style::default().add_modifier(Modifier::BOLD));

        let mut table_rows: Vec<Row> = Vec::new();

        for (i, node_item) in state.items.items.iter().enumerate() {
            let is_selected = state.items.state.selected() == Some(i);
            let status = format_status_cell(node_item, status_column_width);
            let node_name = if reachability_active {
                truncate_to_width(&node_item.service_name, NODE_WIDTH)
            } else {
                node_item.service_name.clone()
            };

            let row_style = if node_item.is_locked() {
                // Locked nodes: dimmed appearance, not fully interactive
                if is_selected {
                    Style::default().fg(COOL_GREY).bg(DARK_GUNMETAL)
                } else {
                    Style::default().fg(COOL_GREY)
                }
            } else if is_selected {
                Style::default().fg(GHOST_WHITE).bg(INDIGO)
            } else {
                Style::default().fg(GHOST_WHITE)
            };

            let row_data = vec![
                node_name,
                node_item.version.clone(),
                format!(
                    "{:>width$}",
                    node_item.rewards_wallet_balance,
                    width = ATTOS_WIDTH
                ),
                format!(
                    "{:>width$} MB",
                    node_item.memory,
                    width = MEMORY_WIDTH.saturating_sub(3)
                ),
                format!("{:>width$}", node_item.mbps, width = MBPS_WIDTH),
                format!("{:>width$}", node_item.records, width = RECORDS_WIDTH),
                format!("{:>width$}", node_item.peers, width = PEERS_WIDTH),
                format!("{:>width$}", node_item.connections, width = CONNS_WIDTH),
                status,
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
    }

    fn render_spinners(
        self,
        table_area: Rect,
        f: &mut crate::tui::Frame<'_>,
        state: &mut NodeTableState,
    ) {
        use super::node_item::NodeDisplayStatus;

        // Calculate the spinner column position (rightmost column)
        let spinner_x = table_area.right().saturating_sub(2);

        // Start after the header row (y + 1)
        let start_y = table_area.y + 1;

        for (i, node_item) in state.items.items.iter().enumerate() {
            if i >= state.spinner_states.len() {
                break;
            }

            let spinner_area = Rect::new(spinner_x, start_y + i as u16, 1, 1);

            match node_item.node_display_status {
                NodeDisplayStatus::Running => render_spinner(
                    f,
                    spinner_area,
                    &mut state.spinner_states[i],
                    node_item.is_locked(),
                    SpinnerStyle::Running,
                ),
                NodeDisplayStatus::Starting | NodeDisplayStatus::ReachabilityCheck => {
                    render_spinner(
                        f,
                        spinner_area,
                        &mut state.spinner_states[i],
                        node_item.is_locked(),
                        SpinnerStyle::Starting,
                    )
                }
                NodeDisplayStatus::Stopping => render_spinner(
                    f,
                    spinner_area,
                    &mut state.spinner_states[i],
                    node_item.is_locked(),
                    SpinnerStyle::Stopping,
                ),
                NodeDisplayStatus::Stopped => render_spinner(
                    f,
                    spinner_area,
                    &mut state.spinner_states[i],
                    node_item.is_locked(),
                    SpinnerStyle::Stopped,
                ),
                NodeDisplayStatus::Unreachable => {
                    let symbol = if node_item.is_locked() { "!" } else { "X" };
                    let style = Style::default().fg(COOL_GREY).add_modifier(Modifier::BOLD);
                    f.render_widget(Paragraph::new(symbol).style(style), spinner_area);
                }
                _ => render_spinner(
                    f,
                    spinner_area,
                    &mut state.spinner_states[i],
                    node_item.is_locked(),
                    SpinnerStyle::Idle,
                ),
            }
        }
    }
}

enum SpinnerStyle {
    Running,
    Starting,
    Stopping,
    Stopped,
    Idle,
}

fn render_spinner(
    f: &mut crate::tui::Frame<'_>,
    area: Rect,
    state: &mut ThrobberState,
    locked: bool,
    style: SpinnerStyle,
) {
    let spinner = match style {
        SpinnerStyle::Running => Throbber::default()
            .throbber_style(if locked {
                Style::default().fg(COOL_GREY).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(EUCALYPTUS).add_modifier(Modifier::BOLD)
            })
            .throbber_set(throbber_widgets_tui::BRAILLE_SIX_DOUBLE)
            .use_type(WhichUse::Spin),
        SpinnerStyle::Starting => Throbber::default()
            .throbber_style(if locked {
                Style::default().fg(COOL_GREY).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(EUCALYPTUS).add_modifier(Modifier::BOLD)
            })
            .throbber_set(throbber_widgets_tui::BOX_DRAWING)
            .use_type(WhichUse::Spin),
        SpinnerStyle::Stopping => Throbber::default()
            .throbber_style(if locked {
                Style::default().fg(COOL_GREY).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(EUCALYPTUS).add_modifier(Modifier::BOLD)
            })
            .throbber_set(throbber_widgets_tui::CLOCK)
            .use_type(WhichUse::Spin),
        SpinnerStyle::Stopped => Throbber::default()
            .throbber_style(
                Style::default()
                    .fg(if locked { COOL_GREY } else { GHOST_WHITE })
                    .add_modifier(Modifier::BOLD),
            )
            .throbber_set(throbber_widgets_tui::BRAILLE_SIX_DOUBLE)
            .use_type(WhichUse::Full),
        SpinnerStyle::Idle => Throbber::default()
            .throbber_style(Style::default().fg(if locked { COOL_GREY } else { GHOST_WHITE }))
            .use_type(WhichUse::Full),
    };

    f.render_stateful_widget(spinner, area, state);
}

fn format_status_cell(node_item: &NodeItem, status_width: usize) -> String {
    let status_width = status_width.max(1);
    let text = match node_item.node_display_status {
        NodeDisplayStatus::Unreachable => node_item
            .last_critical_failure
            .as_ref()
            .map(|log| truncate_to_width(&log.reason, status_width))
            .unwrap_or_else(|| "Unreachable".to_string()),
        NodeDisplayStatus::ReachabilityCheck => match node_item.reachability_progress {
            ReachabilityProgress::InProgress(percent) => {
                format_startup_check(percent, status_width)
            }
            ReachabilityProgress::Complete => {
                truncate_to_width("Startup check complete".to_string(), status_width)
            }
            ReachabilityProgress::NotRun => {
                truncate_to_width("Startup check".to_string(), status_width)
            }
        },
        NodeDisplayStatus::Running => format_running_status(node_item, status_width),
        _ => truncate_to_width(node_item.node_display_status.to_string(), status_width),
    };

    pad_to_width(text, status_width)
}

fn format_reachability_status(values: &ReachabilityStatusValues) -> String {
    let mut modes = Vec::new();
    if values.public {
        modes.push("Public");
    }
    if values.private {
        modes.push("Private");
    }
    if values.upnp {
        modes.push("UPnP");
    }
    if modes.is_empty() {
        "Unknown".to_string()
    } else {
        modes.join(", ")
    }
}

fn format_startup_check(percent: u8, status_width: usize) -> String {
    if status_width == 0 {
        return String::new();
    }

    let label = STARTUP_CHECK_LABEL;
    let label_len = label.len();
    let percent_text = format!("{percent}%");
    let percent_len = percent_text.len();

    if status_width < label_len {
        return pad_to_width(truncate_to_width(label, status_width), status_width);
    }

    if status_width < label_len + 1 + percent_len {
        return pad_to_width(label, status_width);
    }

    let base = format!("{label} {percent_text}");
    if status_width <= base.len() {
        return pad_to_width(base, status_width);
    }

    let minimal_with_bar = base.len() + 1 + MIN_BAR_WIDTH;
    if status_width < minimal_with_bar {
        return pad_to_width(base, status_width);
    }

    let available_for_bar = status_width - base.len() - 1;
    let bar_width = available_for_bar.min(MAX_BAR_WIDTH);
    if let Some(bar) = make_progress_bar(percent, bar_width) {
        return pad_to_width(format!("{label} {percent_text} {bar}"), status_width);
    }

    pad_to_width(base, status_width)
}

fn format_running_status(node_item: &NodeItem, status_width: usize) -> String {
    let mut status_text = truncate_to_width("Running", status_width);
    let remaining = status_width.saturating_sub(status_text.len());

    match node_item.reachability_progress {
        ReachabilityProgress::Complete => {
            let modes = format_reachability_status(&node_item.reachability_status);
            if modes != "Unknown" {
                let addition = format!(" • {modes}");
                if addition.len() <= remaining {
                    status_text.push_str(&addition);
                }
            }
        }
        ReachabilityProgress::InProgress(percent) => {
            if remaining > 1 {
                let available_for_bar = remaining - 1;
                let bar_width = available_for_bar.min(MAX_BAR_WIDTH);
                if bar_width >= MIN_BAR_WIDTH {
                    if let Some(bar) = make_progress_bar(percent, bar_width) {
                        status_text.push(' ');
                        status_text.push_str(&bar);
                    }
                } else if let Some(bar) = make_progress_bar(percent, bar_width) {
                    status_text.push(' ');
                    status_text.push_str(&bar);
                }
            }
        }
        ReachabilityProgress::NotRun => {}
    }

    pad_to_width(status_text, status_width)
}

fn truncate_to_width(value: impl AsRef<str>, width: usize) -> String {
    let value = value.as_ref();
    if value.len() <= width {
        value.to_string()
    } else if width == 0 {
        String::new()
    } else {
        value.chars().take(width).collect()
    }
}

fn pad_to_width(value: impl AsRef<str>, width: usize) -> String {
    let mut truncated = truncate_to_width(value, width);
    let current_len = truncated.len();
    if current_len < width {
        truncated.push_str(&" ".repeat(width - current_len));
    }
    truncated
}

fn make_progress_bar(percent: u8, width: usize) -> Option<String> {
    if width < 3 {
        return None;
    }

    let inner_width = width - 2;
    let filled = ((percent as usize) * inner_width / 100).min(inner_width);
    let mut bar = String::with_capacity(width);
    bar.push('[');
    for _ in 0..filled {
        bar.push('#');
    }
    for _ in filled..inner_width {
        bar.push('.');
    }
    bar.push(']');
    Some(bar)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::node_table::node_item::{NodeDisplayStatus, NodeItem};
    use ant_service_management::ServiceStatus;

    fn node_item_template() -> NodeItem {
        NodeItem {
            service_name: "antnode-1".to_string(),
            version: "0.1.0".to_string(),
            rewards_wallet_balance: 0,
            memory: 0,
            mbps: "0".to_string(),
            records: 0,
            peers: 0,
            connections: 0,
            reachability_progress: ReachabilityProgress::NotRun,
            reachability_status: ReachabilityStatusValues::default(),
            last_critical_failure: None,
            locked: false,
            node_display_status: NodeDisplayStatus::Running,
            service_status: ServiceStatus::Running,
        }
    }

    #[test]
    fn status_cell_hides_progress_for_non_running_services() {
        let mut node = node_item_template();
        node.service_status = ServiceStatus::Stopped;
        node.node_display_status = NodeDisplayStatus::Stopped;
        node.reachability_progress = ReachabilityProgress::InProgress(42);

        let cell = format_status_cell(&node, STATUS_WIDTH);
        assert_eq!(cell.trim_end(), "Stopped");
        assert!(!cell.contains('['));
    }

    #[test]
    fn status_cell_shows_progress_during_reachability_check() {
        let mut node = node_item_template();
        node.node_display_status = NodeDisplayStatus::ReachabilityCheck;
        node.reachability_progress = ReachabilityProgress::InProgress(42);

        let cell = format_status_cell(&node, STATUS_WIDTH);
        assert!(cell.contains('['));
        let trimmed = cell.trim_end();
        let percent_index = trimmed.find("42%").expect("percent missing");
        let bar_index = trimmed.find('[').expect("bar missing");
        assert!(percent_index < bar_index);
    }

    #[test]
    fn status_cell_adds_reachability_modes_when_running() {
        let mut node = node_item_template();
        node.reachability_progress = ReachabilityProgress::Complete;
        node.reachability_status.public = true;

        let cell = format_status_cell(&node, STATUS_WIDTH);
        assert!(cell.contains("Running"));
        assert!(cell.contains("Public"));
    }

    #[test]
    fn startup_check_shows_only_label_when_space_is_tight() {
        let width = STARTUP_CHECK_LABEL.len();
        let text = format_startup_check(42, width);
        let trimmed = text.trim_end();

        assert_eq!(trimmed, STARTUP_CHECK_LABEL);
        assert!(!trimmed.contains('%'));
        assert!(!trimmed.contains('['));
    }

    #[test]
    fn startup_check_adds_percent_once_extra_space_available() {
        let width = STARTUP_CHECK_LABEL.len() + 1 + format!("{percent}%", percent = 55).len();
        let text = format_startup_check(55, width);
        let trimmed = text.trim_end();

        assert!(trimmed.ends_with("55%"));
        assert!(trimmed.starts_with(STARTUP_CHECK_LABEL));
        assert!(!trimmed.contains('['));
    }

    #[test]
    fn startup_check_includes_bar_and_percent_when_space_allows() {
        let width = STARTUP_CHECK_LABEL.len() + 1 + 3 + 1 + MIN_BAR_WIDTH; // label + space + percent + space + bar
        let text = format_startup_check(48, width);
        let trimmed = text.trim_end();
        let percent_index = trimmed.find("48%").expect("percent missing");
        let bar_start = trimmed.find('[').expect("bar missing");

        assert!(percent_index < bar_start);
        let bar = &trimmed[bar_start..];
        assert!(bar.starts_with('[') && bar.ends_with(']'));
        assert_eq!(bar.len(), MIN_BAR_WIDTH);
    }

    #[test]
    fn startup_check_caps_bar_length_at_ten_inner_slots() {
        let width = 60;
        let text = format_startup_check(90, width);
        let trimmed = text.trim_end();
        let bar_start = trimmed.find('[').expect("bar missing");
        let bar = &trimmed[bar_start..];

        assert!(bar.starts_with('[') && bar.ends_with(']'));
        assert_eq!(bar.len(), MAX_BAR_WIDTH);
    }

    #[test]
    fn running_status_includes_progress_when_space_allows() {
        let mut node = node_item_template();
        node.reachability_progress = ReachabilityProgress::InProgress(21);

        let cell = format_status_cell(&node, STATUS_WIDTH);
        assert_eq!(cell.len(), STATUS_WIDTH);
        assert!(cell.contains('['));
    }
}
