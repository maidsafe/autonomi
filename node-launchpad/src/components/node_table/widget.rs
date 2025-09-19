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
pub const MAX_STATUS_WIDTH: usize = 44;
pub const SPINNER_WIDTH: usize = 1;
const NUMBER_OF_COLUMNS: usize = 10;
const STARTUP_CHECK_LABEL: &str = "Reachability";
const MIN_BAR_INNER: usize = 3;
const MAX_BAR_INNER: usize = 10;
const MIN_BAR_WIDTH: usize = MIN_BAR_INNER + 2;
const MAX_BAR_WIDTH: usize = MAX_BAR_INNER + 2;

use super::lifecycle::{CommandKind, LifecycleState, NodeViewModel};
use super::state::NodeTableState;
use crate::components::node_table::StatefulTable;
use crate::style::{COOL_GREY, DARK_GUNMETAL, EUCALYPTUS, GHOST_WHITE, INDIGO, LIGHT_PERIWINKLE};
use ant_service_management::{ReachabilityProgress, metric::ReachabilityStatusValues};
use ratatui::{
    buffer::Buffer,
    prelude::*,
    widgets::{Block, Borders, Cell, HighlightSpacing, Padding, Row, StatefulWidget, Table},
};
use throbber_widgets_tui::{Throbber, ThrobberState, WhichUse};

// Re-export config from state module for convenience
pub use super::state::NodeTableConfig;

pub struct NodeTableWidget;

impl NodeTableWidget {
    pub fn render(self, area: Rect, f: &mut crate::tui::Frame<'_>, state: &mut NodeTableState) {
        let node_count = state.controller.view.items.len();
        let block = NodeTableBlock::new(node_count);
        let table_area = block.inner(area);

        let reachability_active = state.controller.view.items.iter().any(|node| {
            matches!(
                node.reachability_progress,
                ReachabilityProgress::InProgress(_)
            )
        });

        let status_width_hint = measure_status_width(&state.controller.view.items);

        let (column_constraints, status_width) =
            compute_layout(table_area.width, reachability_active, status_width_hint);

        let table_widget = NodesTable::new(column_constraints, status_width, reachability_active);
        f.render_stateful_widget(table_widget, table_area, &mut state.controller.view);

        if state.spinner_states.len() < node_count {
            state
                .spinner_states
                .resize_with(node_count, ThrobberState::default);
        } else if state.spinner_states.len() > node_count {
            state.spinner_states.truncate(node_count);
        }

        let spinner_widget = NodeSpinnerColumn::new(&state.controller.view.items);
        f.render_stateful_widget(spinner_widget, table_area, &mut state.spinner_states);

        f.render_widget(block, area);
    }
}

struct NodeTableBlock {
    node_count: usize,
}

impl NodeTableBlock {
    fn new(node_count: usize) -> Self {
        Self { node_count }
    }

    fn block(&self) -> Block<'static> {
        Block::default()
            .title(Line::from(vec![
                Span::styled(" Nodes", Style::default().fg(GHOST_WHITE).bold()),
                Span::styled(
                    format!(" ({}) ", self.node_count),
                    Style::default().fg(LIGHT_PERIWINKLE),
                ),
            ]))
            .padding(Padding::new(1, 1, 0, 0))
            .title_style(Style::default().fg(GHOST_WHITE))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(EUCALYPTUS))
    }

    fn inner(&self, area: Rect) -> Rect {
        self.block().inner(area)
    }
}

impl Widget for NodeTableBlock {
    fn render(self, area: Rect, buf: &mut Buffer) {
        self.block().render(area, buf);
    }
}

fn measure_status_width(items: &[NodeViewModel]) -> usize {
    items
        .iter()
        .map(|item| format_status_cell(item, MAX_STATUS_WIDTH).trim_end().len())
        .max()
        .unwrap_or(STATUS_WIDTH)
        .max(STATUS_WIDTH)
}

fn compute_layout(
    area_width: u16,
    reachability_active: bool,
    status_width_hint: usize,
) -> ([Constraint; NUMBER_OF_COLUMNS], usize) {
    if reachability_active {
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
        let available_for_status = area_width.saturating_sub(fixed_columns + total_spacing);
        let available_width = available_for_status.max(STATUS_WIDTH as u16) as usize;
        let desired_width = status_width_hint.clamp(STATUS_WIDTH, MAX_STATUS_WIDTH);
        let status_column_width = desired_width.min(available_width);

        (
            [
                Constraint::Min(NODE_WIDTH as u16),
                Constraint::Length(VERSION_WIDTH as u16),
                Constraint::Length(ATTOS_WIDTH as u16),
                Constraint::Length(MEMORY_WIDTH as u16),
                Constraint::Length(MBPS_WIDTH as u16),
                Constraint::Length(RECORDS_WIDTH as u16),
                Constraint::Length(PEERS_WIDTH as u16),
                Constraint::Length(CONNS_WIDTH as u16),
                Constraint::Length(status_column_width as u16),
                Constraint::Length(SPINNER_WIDTH as u16),
            ],
            status_column_width,
        )
    } else {
        let status_column_width = status_width_hint.clamp(STATUS_WIDTH, MAX_STATUS_WIDTH);
        (
            [
                Constraint::Min(NODE_WIDTH as u16),
                Constraint::Length(VERSION_WIDTH as u16),
                Constraint::Length(ATTOS_WIDTH as u16),
                Constraint::Length(MEMORY_WIDTH as u16),
                Constraint::Length(MBPS_WIDTH as u16),
                Constraint::Length(RECORDS_WIDTH as u16),
                Constraint::Length(PEERS_WIDTH as u16),
                Constraint::Length(CONNS_WIDTH as u16),
                Constraint::Length(status_column_width as u16),
                Constraint::Length(SPINNER_WIDTH as u16),
            ],
            status_column_width,
        )
    }
}

struct NodesTable {
    column_constraints: [Constraint; NUMBER_OF_COLUMNS],
    status_width: usize,
    reachability_active: bool,
}

impl NodesTable {
    fn new(
        column_constraints: [Constraint; NUMBER_OF_COLUMNS],
        status_width: usize,
        reachability_active: bool,
    ) -> Self {
        Self {
            column_constraints,
            status_width,
            reachability_active,
        }
    }
}

impl StatefulWidget for NodesTable {
    type State = StatefulTable<NodeViewModel>;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        let header = build_header_row();
        let selected = state.state.selected();

        let rows: Vec<Row> = state
            .items
            .iter()
            .enumerate()
            .map(|(index, item)| {
                let row_data = build_row_data(item, self.status_width, self.reachability_active);
                Row::new(row_data).style(row_style(item, selected == Some(index)))
            })
            .collect();

        let table = Table::new(rows, self.column_constraints)
            .header(header)
            .column_spacing(1)
            .row_highlight_style(Style::default().bg(INDIGO))
            .highlight_spacing(HighlightSpacing::Always);

        StatefulWidget::render(table, area, buf, &mut state.state);
    }
}

struct NodeSpinnerColumn<'a> {
    items: &'a [NodeViewModel],
}

impl<'a> NodeSpinnerColumn<'a> {
    fn new(items: &'a [NodeViewModel]) -> Self {
        Self { items }
    }
}

impl StatefulWidget for NodeSpinnerColumn<'_> {
    type State = Vec<ThrobberState>;

    fn render(self, area: Rect, buf: &mut Buffer, states: &mut Self::State) {
        let spinner_x = area.right().saturating_sub(2);
        let start_y = area.y + 1;

        for (index, node_item) in self.items.iter().enumerate() {
            if index >= states.len() {
                break;
            }

            let spinner_area = Rect::new(spinner_x, start_y + index as u16, 1, 1);
            let style = match node_item.lifecycle {
                LifecycleState::Running => SpinnerStyle::Running,
                LifecycleState::Starting | LifecycleState::Adding => SpinnerStyle::Starting,
                LifecycleState::Stopping => SpinnerStyle::Stopping,
                LifecycleState::Removing => SpinnerStyle::Stopping,
                LifecycleState::Unreachable { .. } => SpinnerStyle::Stopped,
                _ => SpinnerStyle::Idle,
            };

            let spinner = spinner_for(style, node_item.locked);
            StatefulWidget::render(spinner, spinner_area, buf, &mut states[index]);
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

fn spinner_for(style: SpinnerStyle, locked: bool) -> Throbber<'static> {
    match style {
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
    }
}

fn build_header_row() -> Row<'static> {
    Row::new(vec![
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
    .style(Style::default().add_modifier(Modifier::BOLD))
}

fn build_row_data(
    node_item: &NodeViewModel,
    status_width: usize,
    reachability_active: bool,
) -> Vec<String> {
    let node_name = if reachability_active {
        truncate_to_width(&node_item.id, NODE_WIDTH)
    } else {
        node_item.id.clone()
    };

    let status = format_status_cell(node_item, status_width);

    vec![
        node_name,
        truncate_to_width(&node_item.version, VERSION_WIDTH),
        format!(
            "{:>width$}",
            node_item.metrics.rewards_wallet_balance,
            width = ATTOS_WIDTH
        ),
        format!(
            "{:>width$} MB",
            node_item.metrics.memory_usage_mb,
            width = MEMORY_WIDTH.saturating_sub(3)
        ),
        format_bandwidth(&node_item.metrics),
        format!(
            "{:>width$}",
            node_item.metrics.records,
            width = RECORDS_WIDTH
        ),
        format!("{:>width$}", node_item.metrics.peers, width = PEERS_WIDTH),
        format!(
            "{:>width$}",
            node_item.metrics.connections,
            width = CONNS_WIDTH
        ),
        status,
    ]
}

fn format_bandwidth(metrics: &super::lifecycle::NodeMetrics) -> String {
    let down = format_bandwidth_rate(metrics.bandwidth_inbound_bps);
    let up = format_bandwidth_rate(metrics.bandwidth_outbound_bps);
    pad_to_width(format!("↓{down} ↑{up}"), MBPS_WIDTH)
}

fn format_bandwidth_rate(bps: f64) -> String {
    if !bps.is_finite() || bps <= 0.0 {
        return " --.-".to_string();
    }

    let mbps = bps / 1_000_000.0;
    if mbps >= 9999.5 {
        "9999+".to_string()
    } else if mbps >= 1000.0 {
        format!("{mbps:5.0}")
    } else if mbps >= 10.0 {
        format!("{mbps:5.1}")
    } else {
        format!("{mbps:5.2}")
    }
}

fn row_style(node_item: &NodeViewModel, is_selected: bool) -> Style {
    if node_item.locked {
        if is_selected {
            Style::default().fg(COOL_GREY).bg(DARK_GUNMETAL)
        } else {
            Style::default().fg(COOL_GREY)
        }
    } else if is_selected {
        Style::default().fg(GHOST_WHITE).bg(INDIGO)
    } else {
        Style::default().fg(GHOST_WHITE)
    }
}

fn format_status_cell(node_item: &NodeViewModel, status_width: usize) -> String {
    let status_width = status_width.max(1);
    if let Some(failure) = node_item.last_failure.as_ref()
        && !matches!(
            node_item.lifecycle,
            LifecycleState::Running | LifecycleState::Starting | LifecycleState::Adding
        )
    {
        let text = truncate_to_width(failure, status_width);
        return pad_to_width(text, status_width);
    }
    if matches!(node_item.pending_command, Some(CommandKind::Maintain)) {
        let text = truncate_to_width("Maintaining", status_width);
        return pad_to_width(text, status_width);
    }
    let text = match node_item.lifecycle {
        LifecycleState::Adding | LifecycleState::Starting => {
            match node_item.reachability_progress {
                ReachabilityProgress::InProgress(percent) => {
                    format_startup_check(percent, status_width)
                }
                ReachabilityProgress::Complete => {
                    truncate_to_width("Reachability complete", status_width)
                }
                ReachabilityProgress::NotRun => {
                    truncate_to_width(STARTUP_CHECK_LABEL, status_width)
                }
            }
        }
        LifecycleState::Running => match node_item.reachability_progress {
            ReachabilityProgress::InProgress(percent) => {
                format_startup_check(percent, status_width)
            }
            _ => format_running_status(node_item, status_width),
        },
        LifecycleState::Stopping => truncate_to_width("Stopping", status_width),
        LifecycleState::Removing => truncate_to_width("Removing", status_width),
        LifecycleState::Stopped => truncate_to_width("Stopped", status_width),
        LifecycleState::Unreachable { ref reason } => {
            let fallback = reason.clone().unwrap_or_else(|| "Failed".to_string());
            truncate_to_width(fallback, status_width)
        }
        LifecycleState::Refreshing => truncate_to_width(&node_item.status, status_width),
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
    let percent_text = format!("{:02}%", percent);
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

fn format_running_status(node_item: &NodeViewModel, status_width: usize) -> String {
    let mut status_text = truncate_to_width("Running", status_width);
    let remaining = status_width.saturating_sub(status_text.len());

    let modes = format_reachability_status(&node_item.reachability_status);
    if modes != "Unknown" {
        let addition = format!(" • {modes}");
        if addition.len() <= remaining {
            status_text.push_str(&addition);
        }
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
    use super::super::lifecycle::NodeMetrics;
    use super::*;

    fn model_template() -> NodeViewModel {
        NodeViewModel {
            id: "antnode-1".to_string(),
            lifecycle: LifecycleState::Running,
            status: "Running".to_string(),
            version: "0.1.0".to_string(),
            reachability_progress: ReachabilityProgress::NotRun,
            reachability_status: ReachabilityStatusValues::default(),
            metrics: NodeMetrics::default(),
            locked: false,
            last_failure: None,
            pending_command: None,
        }
    }

    #[test]
    fn status_cell_shows_progress_during_reachability_check() {
        let mut model = model_template();
        model.lifecycle = LifecycleState::Starting;
        model.reachability_progress = ReachabilityProgress::InProgress(42);

        let cell = format_status_cell(&model, STATUS_WIDTH);
        assert!(cell.contains('['));
        let percent_index = cell.find("42%").expect("percent missing");
        let bar_index = cell.find('[').expect("bar missing");
        assert!(percent_index < bar_index);
    }

    #[test]
    fn status_cell_adds_reachability_modes_when_running() {
        let mut model = model_template();
        let status = ReachabilityStatusValues {
            public: true,
            ..Default::default()
        };
        model.reachability_status = status;

        let cell = format_status_cell(&model, STATUS_WIDTH);
        assert!(cell.contains("Running"));
        assert!(cell.contains("Public"));
    }

    #[test]
    fn startup_check_caps_bar_length_at_ten_inner_slots() {
        let mut model = model_template();
        model.lifecycle = LifecycleState::Starting;
        model.reachability_progress = ReachabilityProgress::InProgress(90);
        let text = format_status_cell(&model, 60);
        let bar_start = text.find('[').expect("bar missing");
        let bar = text.trim_end();
        let bar = &bar[bar_start..];
        assert!(bar.starts_with('[') && bar.ends_with(']'));
        assert_eq!(bar.len(), MAX_BAR_WIDTH);
    }

    #[test]
    fn status_cell_reports_maintaining_while_locked() {
        let mut model = model_template();
        model.pending_command = Some(CommandKind::Maintain);
        model.locked = true;

        let text = format_status_cell(&model, STATUS_WIDTH);
        assert!(text.contains("Maintaining"));
    }

    #[test]
    fn status_cell_prefers_failure_message_when_present() {
        let mut model = model_template();
        model.lifecycle = LifecycleState::Stopped;
        model.last_failure = Some("Error (Unreachable)".to_string());

        let text = format_status_cell(&model, STATUS_WIDTH);
        assert!(text.contains("Error (Unreachable)"));
        assert!(!text.contains("Stopped"));
    }
}
