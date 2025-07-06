// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use std::{default::Default, rc::Rc};

use super::super::utils::centered_rect_fixed;

use color_eyre::Result;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Style, Stylize},
    text::{Line, Span},
    widgets::{
        Block, Borders, HighlightSpacing, List, ListItem, ListState, Padding, Paragraph, Wrap,
    },
};
use strum::IntoEnumIterator;

use crate::{
    action::{Action, OptionsActions},
    components::Component,
    connection_mode::ConnectionMode,
    mode::{InputMode, Scene},
    style::{
        clear_area, COOL_GREY, DARK_GUNMETAL, EUCALYPTUS, GHOST_WHITE, INDIGO, LIGHT_PERIWINKLE,
        VIVID_SKY_BLUE,
    },
};

#[derive(Default)]
enum ChangeConnectionModeState {
    #[default]
    Selection,
    ConfirmChange,
}

#[derive(Default)]
pub struct ChangeConnectionModePopUp {
    active: bool,
    state: ChangeConnectionModeState,
    items: StatefulList<ConnectionModeItem>,
    connection_mode_selection: ConnectionModeItem,
    connection_mode_initial_state: ConnectionModeItem,
    can_select: bool, // If the user can select the connection mode
}

impl ChangeConnectionModePopUp {
    pub fn new(connection_mode: ConnectionMode) -> Result<Self> {
        let mut selected_connection_mode: ConnectionModeItem = ConnectionModeItem::default();
        let connection_modes_items: Vec<ConnectionModeItem> = ConnectionMode::iter()
            .filter(|cm| cm != &ConnectionMode::HomeNetwork)
            .map(|connection_mode_item| ConnectionModeItem {
                connection_mode: connection_mode_item,
                status: if connection_mode == connection_mode_item {
                    selected_connection_mode = ConnectionModeItem {
                        connection_mode: connection_mode_item,
                        status: ConnectionModeStatus::Selected,
                    };
                    ConnectionModeStatus::Selected
                } else {
                    ConnectionModeStatus::NotSelected
                },
            })
            .collect::<Vec<ConnectionModeItem>>();
        debug!("Connection Mode in Config: {:?}", connection_mode);
        let items = StatefulList::with_items(connection_modes_items);
        Ok(Self {
            active: false,
            state: ChangeConnectionModeState::Selection,
            items,
            connection_mode_selection: selected_connection_mode.clone(),
            connection_mode_initial_state: selected_connection_mode.clone(),
            can_select: false,
        })
    }

    // --- Interactions with the List of modes ---

    /// Deselects all modes in the list of items
    ///
    fn deselect_all(&mut self) {
        for item in &mut self.items.items {
            item.status = ConnectionModeStatus::NotSelected;
        }
    }
    /// Assigns to self.connection_mode_selection the selected connection mode in the list
    ///
    fn assign_connection_mode_selection(&mut self) {
        self.deselect_all();
        if let Some(i) = self.items.state.selected() {
            self.items.items[i].status = ConnectionModeStatus::Selected;
            self.connection_mode_selection = self.items.items[i].clone();
        }
    }
    /// Highlights the connection mode that is currently selected in the list of items.
    ///
    fn select_connection_mode(&mut self) {
        self.deselect_all();
        for (index, item) in self.items.items.iter_mut().enumerate() {
            if item.connection_mode == self.connection_mode_selection.connection_mode {
                item.status = ConnectionModeStatus::Selected;
                self.items.state.select(Some(index));
                break;
            }
        }
    }
    /// Returns the highlighted connection mode in the list of items.
    ///
    fn return_selection(&mut self) -> ConnectionModeItem {
        if let Some(i) = self.items.state.selected() {
            return self.items.items[i].clone();
        }
        ConnectionModeItem::default()
    }

    // Draw functions

    fn draw_selection_state(
        &mut self,
        f: &mut crate::tui::Frame<'_>,
        layer_zero: Rect,
        layer_one: Rc<[Rect]>,
    ) -> Paragraph {
        let pop_up_border: Paragraph = Paragraph::new("").block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Connection Mode ")
                .bold()
                .title_style(Style::new().fg(VIVID_SKY_BLUE))
                .padding(Padding::uniform(2))
                .border_style(Style::new().fg(VIVID_SKY_BLUE)),
        );
        clear_area(f, layer_zero);

        let layer_two = Layout::new(
            Direction::Vertical,
            [
                // for the table
                Constraint::Length(10),
                // gap
                Constraint::Length(3),
                // for the buttons
                Constraint::Length(1),
            ],
        )
        .split(layer_one[1]);

        // Connection Mode selector
        let items: Vec<ListItem> = self
            .items
            .items
            .iter()
            .enumerate()
            .map(|(i, connection_mode_item)| {
                connection_mode_item.to_list_item(i, layer_two[0].width as usize)
            })
            .collect();

        let items = List::new(items)
            .block(Block::default().padding(Padding::uniform(1)))
            .highlight_style(Style::default().bg(INDIGO))
            .highlight_spacing(HighlightSpacing::Always);

        f.render_stateful_widget(items, layer_two[0], &mut self.items.state);

        // Dash
        let dash = Block::new()
            .borders(Borders::BOTTOM)
            .border_style(Style::new().fg(GHOST_WHITE));
        f.render_widget(dash, layer_two[1]);

        // Buttons
        let buttons_layer =
            Layout::horizontal(vec![Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(layer_two[2]);

        let button_no = Line::from(vec![Span::styled(
            "Cancel [Esc]",
            Style::default().fg(LIGHT_PERIWINKLE),
        )]);

        f.render_widget(
            Paragraph::new(button_no)
                .block(Block::default().padding(Padding::horizontal(2)))
                .alignment(Alignment::Left),
            buttons_layer[0],
        );

        let button_yes = Line::from(vec![
            Span::styled(
                "Select ",
                if self.can_select {
                    Style::default().fg(EUCALYPTUS)
                } else {
                    Style::default().fg(COOL_GREY)
                },
            ),
            Span::styled("[Enter]", Style::default().fg(LIGHT_PERIWINKLE).bold()),
        ])
        .alignment(Alignment::Right);

        f.render_widget(
            Paragraph::new(button_yes)
                .block(Block::default().padding(Padding::horizontal(2)))
                .alignment(Alignment::Right),
            buttons_layer[1],
        );

        pop_up_border
    }

    fn draw_confirm_change(
        &mut self,
        f: &mut crate::tui::Frame<'_>,
        layer_zero: Rect,
        layer_one: Rc<[Rect]>,
    ) -> Paragraph {
        // layer zero
        let pop_up_border = Paragraph::new("").block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Confirm & Reset ")
                .bold()
                .title_style(Style::new().fg(VIVID_SKY_BLUE))
                .padding(Padding::uniform(2))
                .border_style(Style::new().fg(VIVID_SKY_BLUE)),
        );
        clear_area(f, layer_zero);

        // split into 3 parts, paragraph, dash, buttons
        let layer_two = Layout::new(
            Direction::Vertical,
            [
                // for the text
                Constraint::Length(9),
                // gap
                Constraint::Length(3),
                // for the buttons
                Constraint::Length(1),
            ],
        )
        .split(layer_one[1]);

        let paragraph_text = Paragraph::new(vec![
            Line::from(Span::styled("\n\n", Style::default())),
            Line::from(Span::styled("\n\n", Style::default())),
            Line::from(vec![
                Span::styled(
                    "Changing connection mode will ",
                    Style::default().fg(LIGHT_PERIWINKLE),
                ),
                Span::styled("reset all nodes.", Style::default().fg(GHOST_WHITE)),
            ]),
            Line::from(Span::styled("\n\n", Style::default())),
            Line::from(Span::styled("\n\n", Style::default())),
            Line::from(Span::styled("\n\n", Style::default())),
            Line::from(vec![
                Span::styled("You’ll need to ", Style::default().fg(LIGHT_PERIWINKLE)),
                Span::styled("Add", Style::default().fg(GHOST_WHITE)),
                Span::styled(" and ", Style::default().fg(LIGHT_PERIWINKLE)),
                Span::styled("Start", Style::default().fg(GHOST_WHITE)),
                Span::styled(
                    " them again afterwards. Are you sure you want to continue?",
                    Style::default().fg(LIGHT_PERIWINKLE),
                ),
            ]),
        ])
        .alignment(Alignment::Left)
        .wrap(Wrap { trim: true })
        .block(Block::default().padding(Padding::horizontal(2)));

        f.render_widget(paragraph_text, layer_two[0]);

        let dash = Block::new()
            .borders(Borders::BOTTOM)
            .border_style(Style::new().fg(GHOST_WHITE));
        f.render_widget(dash, layer_two[1]);

        let buttons_layer =
            Layout::horizontal(vec![Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(layer_two[2]);

        let button_no = Line::from(vec![Span::styled(
            "  Cancel [Esc]",
            Style::default().fg(LIGHT_PERIWINKLE),
        )]);
        let button_yes_style = if self.can_select {
            Style::default().fg(EUCALYPTUS)
        } else {
            Style::default().fg(LIGHT_PERIWINKLE)
        };
        f.render_widget(button_no, buttons_layer[0]);

        let button_yes = Line::from(vec![
            Span::styled("Yes, Change Mode ", button_yes_style),
            Span::styled("[Enter]", Style::default().fg(GHOST_WHITE)),
        ]);
        f.render_widget(button_yes, buttons_layer[1]);

        pop_up_border
    }
}

impl Component for ChangeConnectionModePopUp {
    fn handle_key_events(&mut self, key: KeyEvent) -> Result<Vec<Action>> {
        if !self.active {
            return Ok(vec![]);
        }
        let send_back: Vec<Action> = match &self.state {
            ChangeConnectionModeState::Selection => match key.code {
                KeyCode::Enter => {
                    let connection_mode = self.return_selection();
                    self.connection_mode_initial_state = self.connection_mode_selection.clone();
                    if connection_mode.connection_mode == ConnectionMode::CustomPorts {
                        vec![
                            Action::OptionsActions(OptionsActions::UpdateConnectionMode(
                                ConnectionMode::CustomPorts,
                            )),
                            Action::SwitchScene(Scene::ChangePortsPopUp {
                                connection_mode_old_value: Some(
                                    self.connection_mode_initial_state.connection_mode,
                                ),
                            }),
                        ]
                    } else {
                        self.state = ChangeConnectionModeState::ConfirmChange;
                        vec![]
                    }
                }
                KeyCode::Esc => {
                    debug!("Got Esc, switching to Options");
                    vec![Action::SwitchScene(Scene::Options)]
                }
                KeyCode::Up => {
                    if self.items.items.len() > 1 {
                        self.items.previous();
                        let connection_mode = self.return_selection();
                        self.can_select = connection_mode.connection_mode
                            != self.connection_mode_selection.connection_mode;
                    }
                    vec![]
                }
                KeyCode::Down => {
                    if self.items.items.len() > 1 {
                        self.items.next();
                        let connection_mode = self.return_selection();
                        self.can_select = connection_mode.connection_mode
                            != self.connection_mode_selection.connection_mode;
                    }
                    vec![]
                }
                _ => {
                    vec![]
                }
            },
            ChangeConnectionModeState::ConfirmChange => match key.code {
                KeyCode::Enter => {
                    self.state = ChangeConnectionModeState::Selection;
                    // We allow action if we have more than one connection mode and the action is not
                    // over the connection mode already selected
                    let connection_mode = self.return_selection();
                    if connection_mode.connection_mode
                        != self.connection_mode_selection.connection_mode
                    {
                        debug!(
                                        "Got Enter and there's a new selection, storing value and switching to Options"
                                    );
                        debug!("Connection Mode selected: {:?}", connection_mode);
                        self.connection_mode_initial_state = self.connection_mode_selection.clone();
                        self.assign_connection_mode_selection();
                        vec![
                            Action::StoreConnectionMode(
                                self.connection_mode_selection.connection_mode,
                            ),
                            Action::OptionsActions(OptionsActions::UpdateConnectionMode(
                                connection_mode.clone().connection_mode,
                            )),
                            Action::SwitchScene(Scene::Status),
                        ]
                    } else {
                        debug!("Got Enter, but no new selection. We should not do anything");
                        vec![Action::SwitchScene(Scene::ChangeConnectionModePopUp)]
                    }
                }
                KeyCode::Esc => {
                    self.state = ChangeConnectionModeState::Selection;
                    vec![Action::SwitchScene(Scene::Options)]
                }
                _ => {
                    vec![]
                }
            },
        };
        Ok(send_back)
    }

    fn update(&mut self, action: Action) -> Result<Option<Action>> {
        let send_back = match action {
            Action::SwitchScene(scene) => match scene {
                Scene::ChangeConnectionModePopUp => {
                    self.active = true;
                    self.can_select = false;
                    self.select_connection_mode();
                    Some(Action::SwitchInputMode(InputMode::Entry))
                }
                _ => {
                    self.active = false;
                    None
                }
            },
            // Useful when the user has selected a connection mode but didn't confirm it
            Action::OptionsActions(OptionsActions::UpdateConnectionMode(connection_mode)) => {
                self.connection_mode_selection.connection_mode = connection_mode;
                self.select_connection_mode();
                None
            }
            _ => None,
        };
        Ok(send_back)
    }

    fn draw(&mut self, f: &mut crate::tui::Frame<'_>, area: Rect) -> Result<()> {
        if !self.active {
            return Ok(());
        }

        let layer_zero = centered_rect_fixed(52, 15, area);

        let layer_one = Layout::new(
            Direction::Vertical,
            [
                // Padding from title to the table
                Constraint::Length(1),
                // Table
                Constraint::Min(1),
                // for the pop_up_border
                Constraint::Length(1),
            ],
        )
        .split(layer_zero);

        let pop_up_border: Paragraph = match self.state {
            ChangeConnectionModeState::Selection => {
                self.draw_selection_state(f, layer_zero, layer_one)
            }
            ChangeConnectionModeState::ConfirmChange => {
                self.draw_confirm_change(f, layer_zero, layer_one)
            }
        };

        f.render_widget(pop_up_border, layer_zero);

        Ok(())
    }
}

#[derive(Default)]
struct StatefulList<T> {
    state: ListState,
    items: Vec<T>,
    last_selected: Option<usize>,
}

impl<T> StatefulList<T> {
    fn with_items(items: Vec<T>) -> Self {
        StatefulList {
            state: ListState::default(),
            items,
            last_selected: None,
        }
    }

    fn next(&mut self) {
        let i = match self.state.selected() {
            Some(i) => {
                if i >= self.items.len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => self.last_selected.unwrap_or(0),
        };
        self.state.select(Some(i));
    }

    fn previous(&mut self) {
        let i = match self.state.selected() {
            Some(i) => {
                if i == 0 {
                    self.items.len() - 1
                } else {
                    i - 1
                }
            }
            None => self.last_selected.unwrap_or(0),
        };
        self.state.select(Some(i));
    }
}

#[derive(Default, Debug, Copy, Clone)]
enum ConnectionModeStatus {
    Selected,
    #[default]
    NotSelected,
}

#[derive(Default, Debug, Clone)]
pub struct ConnectionModeItem {
    connection_mode: ConnectionMode,
    status: ConnectionModeStatus,
}

impl ConnectionModeItem {
    fn to_list_item(&self, _index: usize, _width: usize) -> ListItem {
        let line = match self.status {
            ConnectionModeStatus::NotSelected => Line::from(vec![
                Span::raw("   "),
                Span::styled(
                    self.connection_mode.to_string(),
                    Style::default().fg(VIVID_SKY_BLUE),
                ),
            ]),
            ConnectionModeStatus::Selected => Line::from(vec![
                Span::styled(" ►", Style::default().fg(EUCALYPTUS)),
                Span::raw(" "),
                Span::styled(
                    self.connection_mode.to_string(),
                    Style::default().fg(VIVID_SKY_BLUE),
                ),
            ]),
        };

        ListItem::new(line).style(Style::default().bg(DARK_GUNMETAL))
    }
}
