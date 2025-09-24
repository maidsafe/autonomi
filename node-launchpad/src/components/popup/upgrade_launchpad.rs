// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use super::super::Component;
use super::super::utils::centered_rect_fixed;
use crate::{
    action::{Action, UpgradeLaunchpadActions},
    focus::{EventResult, FocusManager, FocusTarget},
    mode::{InputMode, Scene},
    style::{EUCALYPTUS, GHOST_WHITE, LIGHT_PERIWINKLE, VIVID_SKY_BLUE, clear_area},
    widgets::hyperlink::Hyperlink,
};
use ant_releases::{AntReleaseRepoActions, ReleaseType};
use color_eyre::Result;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{prelude::*, widgets::*};
use semver::Version;
use std::any::Any;
use std::time::Duration;

#[derive(Debug, Default)]
pub struct UpgradeLaunchpadPopup {
    current_version: Option<String>,
    latest_version: Option<String>,
}

impl Component for UpgradeLaunchpadPopup {
    fn handle_key_events(
        &mut self,
        key: KeyEvent,
        focus_manager: &FocusManager,
    ) -> Result<(Vec<Action>, EventResult)> {
        if !focus_manager.has_focus(&self.focus_target()) {
            return Ok((vec![], EventResult::Ignored));
        }

        match key.code {
            KeyCode::Enter | KeyCode::Esc => {
                info!("User dismissed the LP upgrade notification.");
                let actions = vec![
                    Action::SwitchInputMode(InputMode::Navigation),
                    Action::SwitchScene(Scene::Status),
                ];
                Ok((actions, EventResult::Consumed))
            }
            _ => Ok((vec![], EventResult::Ignored)),
        }
    }

    fn focus_target(&self) -> FocusTarget {
        FocusTarget::UpgradeLaunchpadPopup
    }

    fn update(&mut self, action: Action) -> Result<Option<Action>> {
        let send_back = match action {
            Action::SwitchScene(Scene::UpgradeLaunchpadPopUp) => {
                Some(Action::SwitchInputMode(InputMode::Entry))
            }
            Action::UpgradeLaunchpadActions(update_launchpad_actions) => {
                match update_launchpad_actions {
                    UpgradeLaunchpadActions::UpdateAvailable {
                        current_version,
                        latest_version,
                    } => {
                        info!(
                            "Received UpdateAvailable action with current version: {current_version} and latest version: {latest_version}. Switching to UpgradeLaunchpadPopUp scene."
                        );
                        self.current_version = Some(current_version);
                        self.latest_version = Some(latest_version);
                        Some(Action::SwitchScene(Scene::UpgradeLaunchpadPopUp))
                    }
                }
            }
            _ => None,
        };
        Ok(send_back)
    }

    fn register_action_handler(
        &mut self,
        tx: tokio::sync::mpsc::UnboundedSender<Action>,
    ) -> Result<()> {
        info!("We've received the action sender. Spawning task to check for updates.");
        tokio::spawn(async move {
            loop {
                match check_for_update().await {
                    Ok(Some((latest_version, current_version))) => {
                        if let Err(err) = tx.send(Action::UpgradeLaunchpadActions(
                            UpgradeLaunchpadActions::UpdateAvailable {
                                current_version: current_version.to_string(),
                                latest_version: latest_version.to_string(),
                            },
                        )) {
                            error!(
                                "Error sending UpgradeLaunchpadActions::UpdateAvailable action: {err}"
                            );
                        }
                    }
                    _ => {
                        info!("No new launchpad version available.");
                    }
                };
                info!("Checking for LP update in 12 hours..");
                tokio::time::sleep(Duration::from_secs(12 * 60 * 60)).await;
            }
        });

        Ok(())
    }

    fn draw(&mut self, f: &mut crate::tui::Frame<'_>, area: Rect) -> Result<()> {
        let Some(current_version) = self.current_version.as_ref() else {
            error!(
                "Current version is not set, even though the upgrade popup is active. This is unexpected."
            );
            return Ok(());
        };
        let Some(latest_version) = self.latest_version.as_ref() else {
            error!(
                "Latest version is not set, even though the upgrade popup is active. This is unexpected."
            );
            return Ok(());
        };

        let layer_zero = centered_rect_fixed(60, 15, area);
        let layer_one = Layout::new(
            Direction::Vertical,
            [
                Constraint::Length(2),
                Constraint::Min(1),
                Constraint::Length(1),
            ],
        )
        .split(layer_zero);

        let pop_up_border = Paragraph::new("").block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Update Available ")
                .bold()
                .title_style(Style::new().fg(VIVID_SKY_BLUE))
                .padding(Padding::uniform(2))
                .border_style(Style::new().fg(VIVID_SKY_BLUE)),
        );
        clear_area(f, layer_zero);

        let layer_two = Layout::new(
            Direction::Vertical,
            [
                Constraint::Length(6),
                Constraint::Length(2),
                Constraint::Length(3),
                Constraint::Length(1),
            ],
        )
        .split(layer_one[1]);

        let text = Paragraph::new(vec![
            Line::from(Span::styled("\n", Style::default())),
            Line::from(vec![Span::styled(
                "A new version of Node Launchpad is available:".to_string(),
                Style::default().fg(LIGHT_PERIWINKLE),
            )]),
            Line::from(vec![Span::styled(
                format!("v{current_version} → v{latest_version}"),
                Style::default().fg(LIGHT_PERIWINKLE),
            )]),
            Line::from(Span::styled("\n", Style::default())),
            Line::from(vec![Span::styled(
                "To update, please download the latest version from:",
                Style::default().fg(GHOST_WHITE),
            )]),
        ])
        .block(Block::default().padding(Padding::horizontal(2)))
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: true });

        f.render_widget(text, layer_two[0]);

        // Center the link in its own layout
        let link_layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(28), // Left margin
                Constraint::Percentage(44), // Link
                Constraint::Percentage(28), // Right margin
            ])
            .split(layer_two[1]);

        // Render hyperlink with proper spacing and alignment
        let link = Hyperlink::new(
            Span::styled(
                "https://autonomi.com/node",
                Style::default().fg(VIVID_SKY_BLUE),
            ),
            "https://autonomi.com/node",
        );
        // Use render_widget_ref for hyperlinks to render correctly
        f.render_widget_ref(link, link_layout[1]);

        let dash = Block::new()
            .borders(Borders::BOTTOM)
            .border_style(Style::new().fg(GHOST_WHITE));
        f.render_widget(dash, layer_two[2]);

        let buttons_layer =
            Layout::horizontal(vec![Constraint::Percentage(100)]).split(layer_two[3]);

        let button_ok = Paragraph::new(Line::from(vec![Span::styled(
            "Press [Enter] to continue",
            Style::default().fg(EUCALYPTUS),
        )]))
        .alignment(Alignment::Center);
        f.render_widget(button_ok, buttons_layer[0]);

        f.render_widget(pop_up_border, layer_zero);

        Ok(())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// Checks if an update is available.
/// Return New, Current version if available.
pub async fn check_for_update() -> Result<Option<(Version, Version)>> {
    let release_repo = <dyn AntReleaseRepoActions>::default_config();
    let current_version = Version::parse(env!("CARGO_PKG_VERSION"))?;

    match release_repo
        .get_latest_version(&ReleaseType::NodeLaunchpad)
        .await
    {
        Ok(latest_version) => {
            info!("Current version: {current_version} and latest version: {latest_version}");
            if latest_version > current_version {
                Ok(Some((latest_version, current_version)))
            } else {
                Ok(None)
            }
        }
        Err(e) => {
            debug!("Failed to check for updates: {}", e);
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::focus::FocusManager;
    use crossterm::event::KeyModifiers;

    #[test]
    fn handle_key_events_requires_focus() {
        let mut popup = UpgradeLaunchpadPopup::default();
        let focus_manager = FocusManager::new(FocusTarget::Status);
        let (actions, result) = popup
            .handle_key_events(
                KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()),
                &focus_manager,
            )
            .expect("handled");
        assert!(actions.is_empty());
        assert_eq!(result, EventResult::Ignored);
    }

    #[test]
    fn handle_key_events_returns_to_status() {
        let mut popup = UpgradeLaunchpadPopup::default();
        let focus_manager = FocusManager::new(FocusTarget::UpgradeLaunchpadPopup);
        let (actions, result) = popup
            .handle_key_events(
                KeyEvent::new(KeyCode::Esc, KeyModifiers::empty()),
                &focus_manager,
            )
            .expect("handled");
        assert_eq!(result, EventResult::Consumed);
        assert!(actions.contains(&Action::SwitchScene(Scene::Status)));
        assert!(actions.contains(&Action::SwitchInputMode(InputMode::Navigation)));
    }

    #[test]
    fn update_available_switches_scene_and_stores_versions() {
        let mut popup = UpgradeLaunchpadPopup::default();
        let action = popup
            .update(Action::UpgradeLaunchpadActions(
                UpgradeLaunchpadActions::UpdateAvailable {
                    current_version: "0.1.0".into(),
                    latest_version: "0.2.0".into(),
                },
            ))
            .expect("update")
            .expect("action");
        assert_eq!(action, Action::SwitchScene(Scene::UpgradeLaunchpadPopUp));
        assert_eq!(popup.current_version.as_deref(), Some("0.1.0"));
        assert_eq!(popup.latest_version.as_deref(), Some("0.2.0"));
    }

    #[test]
    fn switch_scene_enters_entry_mode() {
        let mut popup = UpgradeLaunchpadPopup::default();
        let action = popup
            .update(Action::SwitchScene(Scene::UpgradeLaunchpadPopUp))
            .expect("update")
            .expect("action");
        assert_eq!(action, Action::SwitchInputMode(InputMode::Entry));
    }
}
