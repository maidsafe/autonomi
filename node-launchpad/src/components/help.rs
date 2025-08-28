// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use super::header::SelectedMenuItem;
use color_eyre::eyre::Result;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Style, Stylize},
    text::Span,
    widgets::{Block, Borders, Padding},
};
use tokio::sync::mpsc::UnboundedSender;

use super::Component;
use crate::{
    action::Action,
    components::header::Header,
    mode::{InputMode, Scene},
    style::{COOL_GREY, GHOST_WHITE, VIVID_SKY_BLUE},
    widgets::hyperlink::Hyperlink,
};

#[derive(Clone)]
pub struct Help {
    pub action_tx: Option<UnboundedSender<Action>>,
}

impl Help {
    pub async fn new() -> Result<Self> {
        Ok(Self { action_tx: None })
    }
}

impl Component for Help {
    fn init(&mut self, _area: Rect) -> Result<()> {
        Ok(())
    }

    fn draw(&mut self, f: &mut Frame<'_>, area: Rect) -> Result<()> {
        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints(vec![Constraint::Length(1), Constraint::Length(9)])
            .split(area);

        // ==== Header =====
        let header = Header::new();
        f.render_stateful_widget(header, layout[0], &mut SelectedMenuItem::Help);

        // ---- Get Help & Support ----
        // Links

        // Create a new layout as a table, so we can render hyperlinks
        let columns_layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(vec![Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(layout[1]);

        let padded_area_left = Rect {
            x: columns_layout[0].x + 2,
            y: columns_layout[0].y + 2,
            width: columns_layout[0].width - 2,
            height: columns_layout[0].height - 2,
        };

        let left_column = Layout::default()
            .direction(Direction::Vertical)
            .constraints(vec![
                Constraint::Max(1),
                Constraint::Max(2),
                Constraint::Max(1),
                Constraint::Max(2),
            ])
            .split(padded_area_left);

        let padded_area_right = Rect {
            x: columns_layout[1].x + 2,
            y: columns_layout[1].y + 2,
            width: columns_layout[1].width - 2,
            height: columns_layout[1].height - 2,
        };
        let right_column = Layout::default()
            .direction(Direction::Vertical)
            .constraints(vec![
                Constraint::Max(1),
                Constraint::Max(2),
                Constraint::Max(1),
                Constraint::Max(2),
            ])
            .split(padded_area_right);

        let quickstart_guide_link = Hyperlink::new(
            Span::styled(
                "autonomi.com/getstarted",
                Style::default().fg(VIVID_SKY_BLUE).underlined(),
            ),
            "https://autonomi.com/getstarted",
        );
        let terms_and_conditions_link = Hyperlink::new(
            Span::styled(
                "autonomi.com/terms",
                Style::default().fg(VIVID_SKY_BLUE).underlined(),
            ),
            "https://autonomi.com/terms",
        );
        let get_direct_support_link = Hyperlink::new(
            Span::styled(
                "autonomi.com/support",
                Style::default().fg(VIVID_SKY_BLUE).underlined(),
            ),
            "https://autonomi.com/support",
        );
        let download_latest_link = Hyperlink::new(
            Span::styled(
                "autonomi.com/downloads",
                Style::default().fg(VIVID_SKY_BLUE).underlined(),
            ),
            "https://autonomi.com/downloads",
        );

        let block = Block::new()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(COOL_GREY))
            .padding(Padding::uniform(1))
            .title(" Get Help & Support ")
            .bold()
            .title_style(Style::default().bold().fg(GHOST_WHITE));

        // Render hyperlinks in the new area
        f.render_widget(
            Span::styled(
                "Read the quick start guides:",
                Style::default().fg(GHOST_WHITE),
            ),
            left_column[0],
        );
        f.render_widget_ref(quickstart_guide_link, left_column[1]);
        f.render_widget(
            Span::styled("Get Direct Support:", Style::default().fg(GHOST_WHITE)),
            left_column[2],
        );
        f.render_widget_ref(get_direct_support_link, left_column[3]);
        f.render_widget(
            Span::styled(
                "Download the latest launchpad:",
                Style::default().fg(GHOST_WHITE),
            ),
            right_column[0],
        );
        f.render_widget_ref(download_latest_link, right_column[1]);
        f.render_widget(
            Span::styled("Terms & Conditions:", Style::default().fg(GHOST_WHITE)),
            right_column[2],
        );
        f.render_widget_ref(terms_and_conditions_link, right_column[3]);

        f.render_widget(block, layout[1]);

        Ok(())
    }

    fn update(&mut self, action: Action) -> Result<Option<Action>> {
        if let Action::SwitchScene(Scene::Help) = action {
            // make sure we're in navigation mode
            return Ok(Some(Action::SwitchInputMode(InputMode::Navigation)));
        }
        Ok(None)
    }

    fn focus_target(&self) -> crate::focus::FocusTarget {
        crate::focus::FocusTarget::Help
    }
}
