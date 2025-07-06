// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::style::{GHOST_WHITE, LIGHT_PERIWINKLE, VIVID_SKY_BLUE};
use ratatui::{prelude::*, widgets::*};

pub enum SelectedMenuItem {
    Status,
    Options,
    Help,
}

pub struct Header {
    launchpad_version_str: String,
}

impl Default for Header {
    fn default() -> Self {
        let version_str = env!("CARGO_PKG_VERSION");
        Self {
            launchpad_version_str: version_str.to_string(),
        }
    }
}

impl Header {
    pub fn new() -> Self {
        Self::default()
    }
}

impl StatefulWidget for Header {
    type State = SelectedMenuItem;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints(vec![Constraint::Length(1)])
            .split(area);

        // Define content of the header
        let application_text = Span::styled(
            format!(" Autonomi Node Launchpad (v{})", self.launchpad_version_str),
            Style::default().fg(LIGHT_PERIWINKLE),
        );

        // Determine the color for each part of the menu based on the state
        let status_color = if matches!(state, SelectedMenuItem::Status) {
            VIVID_SKY_BLUE
        } else {
            GHOST_WHITE
        };

        let options_color = if matches!(state, SelectedMenuItem::Options) {
            VIVID_SKY_BLUE
        } else {
            GHOST_WHITE
        };

        let help_color = if matches!(state, SelectedMenuItem::Help) {
            VIVID_SKY_BLUE
        } else {
            GHOST_WHITE
        };

        // Create styled spans for each part of the menu
        let status = Span::styled("[S]tatus", Style::default().fg(status_color));
        let options = Span::styled("[O]ptions", Style::default().fg(options_color));
        let help = Span::styled("[H]elp", Style::default().fg(help_color));

        // Combine the menu parts with separators
        let menu = vec![
            status,
            Span::raw(" | ").fg(VIVID_SKY_BLUE),
            options,
            Span::raw(" | ").fg(VIVID_SKY_BLUE),
            help,
        ];

        // Calculate spacing between title and menu items
        let total_width = (layout[0].width - 1) as usize;
        let spaces = " ".repeat(total_width.saturating_sub(
            application_text.content.len() + menu.iter().map(|s| s.width()).sum::<usize>(),
        ));

        // Create a line with left and right text
        let line = Line::from(
            vec![application_text, Span::raw(spaces)]
                .into_iter()
                .chain(menu)
                .collect::<Vec<_>>(),
        );

        // Create a Paragraph widget to display the line
        let paragraph = Paragraph::new(line).block(Block::default().borders(Borders::NONE));

        paragraph.render(layout[0], buf);
    }
}
