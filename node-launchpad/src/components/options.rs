use std::{cmp::max, path::PathBuf};

use color_eyre::eyre::Result;
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Row, Table},
    Frame,
};
use tokio::sync::mpsc::UnboundedSender;

use super::{header::SelectedMenuItem, utils::open_logs, Component};
use crate::{
    action::{Action, OptionsActions},
    components::header::Header,
    connection_mode::ConnectionMode,
    mode::{InputMode, Scene},
    style::{
        COOL_GREY, EUCALYPTUS, GHOST_WHITE, LIGHT_PERIWINKLE, VERY_LIGHT_AZURE, VIVID_SKY_BLUE,
    },
};

#[derive(Clone)]
pub struct Options {
    pub storage_mountpoint: PathBuf,
    pub storage_drive: String,
    pub rewards_address: String,
    pub connection_mode: ConnectionMode,
    pub port_edit: bool,
    pub port_from: Option<u32>,
    pub port_to: Option<u32>,
    pub active: bool,
    pub action_tx: Option<UnboundedSender<Action>>,
}

impl Options {
    pub async fn new(
        storage_mountpoint: PathBuf,
        storage_drive: String,
        rewards_address: String,
        connection_mode: ConnectionMode,
        port_from: Option<u32>,
        port_to: Option<u32>,
    ) -> Result<Self> {
        Ok(Self {
            storage_mountpoint,
            storage_drive,
            rewards_address,
            connection_mode,
            port_edit: false,
            port_from,
            port_to,
            active: false,
            action_tx: None,
        })
    }
}

impl Component for Options {
    fn draw(&mut self, f: &mut Frame<'_>, area: Rect) -> Result<()> {
        if !self.active {
            return Ok(());
        }
        // Define the layout to split the area into four sections
        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints(
                [
                    Constraint::Length(1),
                    Constraint::Length(5),
                    Constraint::Length(3),
                    Constraint::Length(3),
                    Constraint::Length(4),
                    Constraint::Length(3),
                ]
                .as_ref(),
            )
            .split(area);

        // ==== Header =====
        let header = Header::new();
        f.render_stateful_widget(header, layout[0], &mut SelectedMenuItem::Options);

        // Storage Drive
        let port_legend = " Edit Port Range ";
        let port_key = " [Ctrl+P] ";
        let block1 = Block::default()
            .title(" Device Options ")
            .title_style(Style::default().bold().fg(GHOST_WHITE))
            .style(Style::default().fg(GHOST_WHITE))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(VERY_LIGHT_AZURE));
        let storage_drivename = Table::new(
            vec![
                Row::new(vec![
                    Cell::from(
                        Line::from(vec![Span::styled(
                            " Storage Drive: ",
                            Style::default().fg(LIGHT_PERIWINKLE),
                        )])
                        .alignment(Alignment::Left),
                    ),
                    Cell::from(
                        Line::from(vec![Span::styled(
                            format!(" {} ", self.storage_drive),
                            Style::default().fg(VIVID_SKY_BLUE),
                        )])
                        .alignment(Alignment::Left),
                    ),
                    Cell::from(
                        Line::from(vec![
                            Span::styled(" Change Drive ", Style::default().fg(VERY_LIGHT_AZURE)),
                            Span::styled(" [Ctrl+D] ", Style::default().fg(GHOST_WHITE)),
                        ])
                        .alignment(Alignment::Right),
                    ),
                ]),
                Row::new(vec![
                    Cell::from(
                        Line::from(vec![Span::styled(
                            " Connection Mode: ",
                            Style::default().fg(LIGHT_PERIWINKLE),
                        )])
                        .alignment(Alignment::Left),
                    ),
                    Cell::from(
                        Line::from(vec![Span::styled(
                            format!(" {} ", self.connection_mode),
                            Style::default().fg(VIVID_SKY_BLUE),
                        )])
                        .alignment(Alignment::Left),
                    ),
                    Cell::from(
                        Line::from(vec![
                            Span::styled(" Change Mode ", Style::default().fg(VERY_LIGHT_AZURE)),
                            Span::styled(" [Ctrl+K] ", Style::default().fg(GHOST_WHITE)),
                        ])
                        .alignment(Alignment::Right),
                    ),
                ]),
                Row::new(vec![
                    Cell::from(
                        Line::from(vec![Span::styled(
                            " Port Range: ",
                            Style::default().fg(LIGHT_PERIWINKLE),
                        )])
                        .alignment(Alignment::Left),
                    ),
                    Cell::from(
                        Line::from(vec![
                            if self.connection_mode == ConnectionMode::CustomPorts {
                                Span::styled(
                                    format!(
                                        " {}-{} ",
                                        self.port_from.unwrap_or(0),
                                        self.port_to.unwrap_or(0)
                                    ),
                                    Style::default().fg(VIVID_SKY_BLUE),
                                )
                            } else {
                                Span::styled(" Auto ", Style::default().fg(COOL_GREY))
                            },
                        ])
                        .alignment(Alignment::Left),
                    ),
                    Cell::from(
                        Line::from(if self.connection_mode == ConnectionMode::CustomPorts {
                            vec![
                                Span::styled(port_legend, Style::default().fg(VERY_LIGHT_AZURE)),
                                Span::styled(port_key, Style::default().fg(GHOST_WHITE)),
                            ]
                        } else {
                            vec![]
                        })
                        .alignment(Alignment::Right),
                    ),
                ]),
            ],
            &[
                Constraint::Length(18),
                Constraint::Fill(1),
                Constraint::Length((port_legend.len() + port_key.len()) as u16),
            ],
        )
        .block(block1)
        .style(Style::default().fg(GHOST_WHITE));

        // Beta Rewards Program
        let beta_legend = if self.rewards_address.is_empty() {
            " Add Wallet "
        } else {
            " Change Wallet "
        };
        let beta_key = " [Ctrl+B] ";
        let block2 = Block::default()
            .title(" Wallet ")
            .title_style(Style::default().bold().fg(GHOST_WHITE))
            .style(Style::default().fg(GHOST_WHITE))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(VERY_LIGHT_AZURE));
        let beta_rewards = Table::new(
            vec![Row::new(vec![
                Cell::from(
                    Line::from(vec![Span::styled(
                        " Wallet Address: ",
                        Style::default().fg(LIGHT_PERIWINKLE),
                    )])
                    .alignment(Alignment::Left),
                ),
                Cell::from(
                    Line::from(vec![Span::styled(
                        format!(" {} ", self.rewards_address),
                        Style::default().fg(VIVID_SKY_BLUE),
                    )])
                    .alignment(Alignment::Left),
                ),
                Cell::from(
                    Line::from(vec![
                        Span::styled(beta_legend, Style::default().fg(VERY_LIGHT_AZURE)),
                        Span::styled(beta_key, Style::default().fg(GHOST_WHITE)),
                    ])
                    .alignment(Alignment::Right),
                ),
            ])],
            &[
                Constraint::Length(18),
                Constraint::Fill(1),
                Constraint::Length((beta_legend.len() + beta_key.len()) as u16),
            ],
        )
        .block(block2)
        .style(Style::default().fg(GHOST_WHITE));

        // Access Logs
        let logs_legend = " Access Logs ";
        let logs_key = " [Ctrl+L] ";
        let block3 = Block::default()
            .title(" Access Logs ")
            .title_style(Style::default().bold().fg(GHOST_WHITE))
            .style(Style::default().fg(GHOST_WHITE))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(VERY_LIGHT_AZURE));
        let logs_folder = Table::new(
            vec![Row::new(vec![
                Cell::from(
                    Line::from(vec![Span::styled(
                        " Open the Logs folder on this device ",
                        Style::default().fg(LIGHT_PERIWINKLE),
                    )])
                    .alignment(Alignment::Left),
                ),
                Cell::from(
                    Line::from(vec![
                        Span::styled(logs_legend, Style::default().fg(VERY_LIGHT_AZURE)),
                        Span::styled(logs_key, Style::default().fg(GHOST_WHITE)),
                    ])
                    .alignment(Alignment::Right),
                ),
            ])],
            &[
                Constraint::Fill(1),
                Constraint::Length((logs_legend.len() + logs_key.len()) as u16),
            ],
        )
        .block(block3)
        .style(Style::default().fg(GHOST_WHITE));

        // Update Nodes
        let reset_legend = " Begin Reset ";
        let reset_key = " [Ctrl+R] ";
        let upgrade_legend = " Begin Upgrade ";
        let upgrade_key = " [Ctrl+U] ";
        let block4 = Block::default()
            .title(" Update Nodes ")
            .title_style(Style::default().bold().fg(GHOST_WHITE))
            .style(Style::default().fg(GHOST_WHITE))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(EUCALYPTUS));
        let reset_nodes = Table::new(
            vec![
                Row::new(vec![
                    Cell::from(
                        Line::from(vec![Span::styled(
                            " Upgrade all Nodes ",
                            Style::default().fg(LIGHT_PERIWINKLE),
                        )])
                        .alignment(Alignment::Left),
                    ),
                    Cell::from(
                        Line::from(vec![
                            Span::styled(upgrade_legend, Style::default().fg(EUCALYPTUS)),
                            Span::styled(upgrade_key, Style::default().fg(GHOST_WHITE)),
                        ])
                        .alignment(Alignment::Right),
                    ),
                ]),
                Row::new(vec![
                    Cell::from(
                        Line::from(vec![Span::styled(
                            " Reset all Nodes on this device ",
                            Style::default().fg(LIGHT_PERIWINKLE),
                        )])
                        .alignment(Alignment::Left),
                    ),
                    Cell::from(
                        Line::from(vec![
                            Span::styled(reset_legend, Style::default().fg(EUCALYPTUS)),
                            Span::styled(reset_key, Style::default().fg(GHOST_WHITE)),
                        ])
                        .alignment(Alignment::Right),
                    ),
                ]),
            ],
            &[
                Constraint::Fill(1),
                Constraint::Length(
                    (max(reset_legend.len(), upgrade_legend.len())
                        + max(reset_key.len(), upgrade_key.len())) as u16,
                ),
            ],
        )
        .block(block4)
        .style(Style::default().fg(GHOST_WHITE));

        // Quit
        let quit_legend = "Quit ";
        let quit_key = "[Q] ";
        let block5 = Block::default()
            .style(Style::default().fg(GHOST_WHITE))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(VIVID_SKY_BLUE));
        let quit = Table::new(
            vec![Row::new(vec![
                Cell::from(
                    Line::from(vec![Span::styled(
                        " Close Launchpad (your nodes will keep running in the background) ",
                        Style::default().fg(LIGHT_PERIWINKLE),
                    )])
                    .alignment(Alignment::Left),
                ),
                Cell::from(
                    Line::from(vec![
                        Span::styled(quit_legend, Style::default().fg(VIVID_SKY_BLUE)),
                        Span::styled(quit_key, Style::default().fg(GHOST_WHITE)),
                    ])
                    .alignment(Alignment::Right),
                ),
            ])],
            &[
                Constraint::Fill(1),
                Constraint::Length((quit_legend.len() + quit_key.len()) as u16),
            ],
        )
        .block(block5)
        .style(Style::default().fg(GHOST_WHITE));

        // Render the tables in their respective sections
        f.render_widget(storage_drivename, layout[1]);
        f.render_widget(beta_rewards, layout[2]);
        f.render_widget(logs_folder, layout[3]);
        f.render_widget(reset_nodes, layout[4]);
        f.render_widget(quit, layout[5]);

        Ok(())
    }

    fn update(&mut self, action: Action) -> Result<Option<Action>> {
        match action {
            Action::SwitchScene(scene) => match scene {
                Scene::Options
                | Scene::ChangeDrivePopUp
                | Scene::ChangeConnectionModePopUp
                | Scene::ChangePortsPopUp { .. }
                | Scene::OptionsRewardsAddressPopUp
                | Scene::ResetNodesPopUp
                | Scene::UpgradeNodesPopUp => {
                    self.active = true;
                    // make sure we're in navigation mode
                    return Ok(Some(Action::SwitchInputMode(InputMode::Navigation)));
                }
                _ => self.active = false,
            },
            Action::OptionsActions(action) => match action {
                OptionsActions::TriggerChangeDrive => {
                    return Ok(Some(Action::SwitchScene(Scene::ChangeDrivePopUp)));
                }
                OptionsActions::UpdateStorageDrive(mountpoint, drive) => {
                    self.storage_mountpoint = mountpoint;
                    self.storage_drive = drive;
                }
                OptionsActions::TriggerChangeConnectionMode => {
                    return Ok(Some(Action::SwitchScene(Scene::ChangeConnectionModePopUp)));
                }
                OptionsActions::UpdateConnectionMode(mode) => {
                    self.connection_mode = mode;
                }
                OptionsActions::TriggerChangePortRange => {
                    return Ok(Some(Action::SwitchScene(Scene::ChangePortsPopUp {
                        connection_mode_old_value: None,
                    })));
                }
                OptionsActions::UpdatePortRange(from, to) => {
                    self.port_from = Some(from);
                    self.port_to = Some(to);
                }
                OptionsActions::TriggerRewardsAddress => {
                    return Ok(Some(Action::SwitchScene(Scene::OptionsRewardsAddressPopUp)));
                }
                OptionsActions::UpdateRewardsAddress(rewards_address) => {
                    self.rewards_address = rewards_address;
                }
                OptionsActions::TriggerAccessLogs => {
                    open_logs(None)?;
                }
                OptionsActions::TriggerUpdateNodes => {
                    return Ok(Some(Action::SwitchScene(Scene::UpgradeNodesPopUp)));
                }
                OptionsActions::TriggerResetNodes => {
                    return Ok(Some(Action::SwitchScene(Scene::ResetNodesPopUp)))
                }
                _ => {}
            },
            _ => {}
        }
        Ok(None)
    }
}
