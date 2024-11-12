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
    Configured,
    NotConfigured,
    Running,
}

#[derive(Default)]
pub struct Footer {}

impl StatefulWidget for Footer {
    type State = NodesToStart;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        let (text_style, command_style) = if matches!(state, NodesToStart::Configured) {
            (
                Style::default().fg(EUCALYPTUS),
                Style::default().fg(GHOST_WHITE),
            )
        } else {
            (
                Style::default().fg(COOL_GREY),
                Style::default().fg(LIGHT_PERIWINKLE),
            )
        };

        let commands = vec![
            Span::styled("[+] ", Style::default().fg(GHOST_WHITE)),
            Span::styled("Add", Style::default().fg(EUCALYPTUS)),
            Span::styled(" ", Style::default()),
            Span::styled("[-] ", Style::default().fg(GHOST_WHITE)),
            Span::styled("Remove", Style::default().fg(EUCALYPTUS)),
            Span::styled(" ", Style::default()),
            Span::styled("[Ctrl+S] ", command_style),
            Span::styled("Start/Stop Node", text_style),
            Span::styled(" ", Style::default()),
            Span::styled("[L] ", command_style),
            Span::styled("Open Logs", Style::default().fg(EUCALYPTUS)),
            Span::styled(" ", Style::default()),
            Span::styled("[Ctrl+X] ", command_style),
            Span::styled(
                "Stop All",
                if matches!(state, NodesToStart::Running) {
                    Style::default().fg(EUCALYPTUS)
                } else {
                    Style::default().fg(COOL_GREY)
                },
            ),
        ];

        let cell1 = Cell::from(Line::from(commands));
        let row = Row::new(vec![cell1]);

        let table = Table::new(vec![row], vec![Constraint::Max(1)])
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(EUCALYPTUS))
                    .padding(Padding::horizontal(1)),
            )
            .widths(vec![Constraint::Fill(1)]);

        StatefulWidget::render(table, area, buf, &mut TableState::default());
    }
}
