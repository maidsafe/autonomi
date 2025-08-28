// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use color_eyre::eyre::Result;
use crossterm::event::{KeyEvent, MouseEvent};
use ratatui::layout::Rect;
use tokio::sync::mpsc::UnboundedSender;

use crate::{
    action::Action,
    config::Config,
    focus::{EventResult, FocusManager, FocusTarget},
    tui::{Event, Frame},
};

pub mod footer;
pub mod header;
pub mod help;
pub mod node_table;
pub mod options;
pub mod popup;
pub mod status;
pub mod utils;

/// `Component` is a trait that represents a visual and interactive element of the user interface.
/// Implementors of this trait can be registered with the main application loop and will be able to receive events,
/// update state, and be rendered on the screen.
pub trait Component {
    /// Register an action handler that can send actions for processing if necessary.
    ///
    /// # Arguments
    ///
    /// * `tx` - An unbounded sender that can send actions.
    ///
    /// # Returns
    ///
    /// * `Result<()>` - An Ok result or an error.
    #[expect(unused_variables)]
    fn register_action_handler(&mut self, tx: UnboundedSender<Action>) -> Result<()> {
        Ok(())
    }
    /// Register a configuration handler that provides configuration settings if necessary.
    ///
    /// # Arguments
    ///
    /// * `config` - Configuration settings.
    ///
    /// # Returns
    ///
    /// * `Result<()>` - An Ok result or an error.
    #[expect(unused_variables)]
    fn register_config_handler(&mut self, config: Config) -> Result<()> {
        Ok(())
    }
    /// Initialize the component with a specified area if necessary.
    ///
    /// # Arguments
    ///
    /// * `area` - Rectangular area to initialize the component within.
    ///
    /// # Returns
    ///
    /// * `Result<()>` - An Ok result or an error.
    fn init(&mut self, _area: Rect) -> Result<()> {
        Ok(())
    }
    /// Handle incoming events and produce actions if necessary.
    ///
    /// # Arguments
    ///
    /// * `event` - An optional event to be processed.
    /// * `focus_manager` - The focus manager to check if this component has focus.
    ///
    /// # Returns
    ///
    /// * `Result<(Vec<Action>, EventResult)>` - Actions and whether the event was consumed.
    fn handle_events(
        &mut self,
        event: Option<Event>,
        focus_manager: &FocusManager,
    ) -> Result<(Vec<Action>, EventResult)> {
        let (actions, result) = match event {
            Some(Event::Key(key_event)) => self.handle_key_events(key_event, focus_manager)?,
            Some(Event::Mouse(mouse_event)) => {
                self.handle_mouse_events(mouse_event, focus_manager)?
            }
            _ => (vec![], EventResult::Ignored),
        };
        Ok((actions, result))
    }
    /// Handle key events and produce actions if necessary.
    ///
    /// # Arguments
    ///
    /// * `key` - A key event to be processed.
    /// * `focus_manager` - The focus manager to check if this component has focus.
    ///
    /// # Returns
    ///
    /// * `Result<(Vec<Action>, EventResult)>` - Actions and whether the event was consumed.
    #[expect(unused_variables)]
    fn handle_key_events(
        &mut self,
        key: KeyEvent,
        focus_manager: &FocusManager,
    ) -> Result<(Vec<Action>, EventResult)> {
        Ok((vec![], EventResult::Ignored))
    }
    /// Handle mouse events and produce actions if necessary.
    ///
    /// # Arguments
    ///
    /// * `mouse` - A mouse event to be processed.
    /// * `focus_manager` - The focus manager to check if this component has focus.
    ///
    /// # Returns
    ///
    /// * `Result<(Vec<Action>, EventResult)>` - Actions and whether the event was consumed.
    #[expect(unused_variables)]
    fn handle_mouse_events(
        &mut self,
        mouse: MouseEvent,
        focus_manager: &FocusManager,
    ) -> Result<(Vec<Action>, EventResult)> {
        Ok((vec![], EventResult::Ignored))
    }
    /// Update the state of the component based on a received action. (REQUIRED)
    ///
    /// # Arguments
    ///
    /// * `action` - An action that may modify the state of the component.
    ///
    /// # Returns
    ///
    /// * `Result<Option<Action>>` - An action to be processed or none.
    #[expect(unused_variables)]
    fn update(&mut self, action: Action) -> Result<Option<Action>> {
        Ok(None)
    }
    /// Render the component on the screen. (REQUIRED)
    ///
    /// # Arguments
    ///
    /// * `f` - A frame used for rendering.
    /// * `area` - The area in which the component should be drawn.
    ///
    /// # Returns
    ///
    /// * `Result<()>` - An Ok result or an error.
    fn draw(&mut self, f: &mut Frame<'_>, area: Rect) -> Result<()>;

    /// Get the focus target for this component.
    ///
    /// # Returns
    ///
    /// * `FocusTarget` - The focus target for this component.
    fn focus_target(&self) -> FocusTarget {
        FocusTarget::Status // Default to Status, components should override this
    }
}
