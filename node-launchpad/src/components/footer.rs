// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::style::{COOL_GREY, EUCALYPTUS, GHOST_WHITE, LIGHT_PERIWINKLE};
use ratatui::{prelude::*, widgets::*};

pub enum NodesToStart {
    Running,
    NotRunning,
    RunningSelected,
    NotRunningSelected,
}

#[derive(Default)]
pub struct Footer {}

impl StatefulWidget for Footer {
    type State = NodesToStart;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints(vec![Constraint::Length(3)])
            .split(area);

        let command_enabled = Style::default().fg(GHOST_WHITE);
        let text_enabled = Style::default().fg(EUCALYPTUS);
        let command_disabled = Style::default().fg(LIGHT_PERIWINKLE);
        let text_disabled = Style::default().fg(COOL_GREY);

        let mut remove_command_style = command_disabled;
        let mut remove_text_style = text_disabled;
        let mut start_stop_command_style = command_disabled;
        let mut start_stop_text_style = text_disabled;
        let mut open_logs_command_style = command_disabled;
        let mut open_logs_text_style = text_disabled;
        let mut stop_all_command_style = command_disabled;
        let mut stop_all_text_style = text_disabled;

        match state {
            NodesToStart::Running => {
                stop_all_command_style = command_enabled;
                stop_all_text_style = text_enabled;
            }
            NodesToStart::RunningSelected => {
                remove_command_style = command_enabled;
                remove_text_style = text_enabled;
                start_stop_command_style = command_enabled;
                start_stop_text_style = text_enabled;
                open_logs_command_style = command_enabled;
                open_logs_text_style = text_enabled;
                stop_all_command_style = command_enabled;
                stop_all_text_style = text_enabled;
            }
            NodesToStart::NotRunning => {}
            NodesToStart::NotRunningSelected => {
                remove_command_style = command_enabled;
                remove_text_style = text_enabled;
                start_stop_command_style = command_enabled;
                start_stop_text_style = text_enabled;
                open_logs_command_style = command_enabled;
                open_logs_text_style = text_enabled;
            }
        }

        let commands = vec![
            Span::styled("[+] ", command_enabled),
            Span::styled("Add", text_enabled),
            Span::styled(" ", Style::default()),
            Span::styled("[-] ", remove_command_style),
            Span::styled("Remove", remove_text_style),
            Span::styled(" ", Style::default()),
            Span::styled("[Ctrl+S] ", start_stop_command_style),
            Span::styled("Start/Stop Node", start_stop_text_style),
            Span::styled(" ", Style::default()),
            Span::styled("[L] ", open_logs_command_style),
            Span::styled("Open Logs", open_logs_text_style),
        ];

        let stop_all = vec![
            Span::styled("[Ctrl+X] ", stop_all_command_style),
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
