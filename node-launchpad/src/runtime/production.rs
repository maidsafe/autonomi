// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! Production runtime implementation for the node launchpad TUI.
//!
//! This module contains the production implementation of the Runtime trait,
//! which integrates with actual terminal I/O and user input.

use super::Runtime;
use crate::tui;
use color_eyre::eyre::Result;
use ratatui::{Frame, prelude::Rect};

/// Production runtime that interfaces with the actual terminal.
///
/// This runtime implementation handles real terminal I/O, user input events,
/// and terminal lifecycle management for production use of the application.
pub struct ProductionRuntime {
    tui: tui::Tui,
}

impl ProductionRuntime {
    /// Creates a new production runtime with the specified tick and frame rates.
    ///
    /// # Arguments
    ///
    /// * `tick_rate` - The rate at which tick events are generated (events per second)
    /// * `frame_rate` - The maximum frame rate for rendering (frames per second)
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying TUI system cannot be initialized.
    pub fn new(tick_rate: f64, frame_rate: f64) -> Result<Self> {
        let tui = tui::Tui::new()?.tick_rate(tick_rate).frame_rate(frame_rate);

        Ok(Self { tui })
    }
}

impl Runtime for ProductionRuntime {
    async fn next_event(&mut self) -> Option<tui::Event> {
        self.tui.next().await
    }

    fn draw(&mut self, render_fn: Box<dyn FnOnce(&mut Frame) + '_>) -> Result<()> {
        self.tui.draw(|frame| render_fn(frame))?;
        Ok(())
    }

    fn enter(&mut self) -> Result<()> {
        self.tui.enter()
    }

    fn exit(&mut self) -> Result<()> {
        self.tui.exit()
    }

    fn suspend(&mut self) -> Result<()> {
        self.tui.suspend()
    }

    fn stop(&mut self) -> Result<()> {
        self.tui.stop()
    }

    fn resize(&mut self, rect: Rect) -> Result<()> {
        self.tui.resize(rect)?;
        Ok(())
    }

    fn size(&self) -> Result<Rect> {
        let size = self.tui.size()?;
        Ok(Rect::new(0, 0, size.width, size.height))
    }

    #[cfg(test)]
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}
