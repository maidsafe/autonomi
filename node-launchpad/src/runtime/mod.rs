// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! Runtime abstraction for the node launchpad application.
//!
//! This module provides a runtime abstraction layer that enables both production
//! and test execution of the TUI application. The `Runtime` trait defines the
//! interface for event handling, rendering, and lifecycle management.

use crate::tui;
use color_eyre::eyre::Result;
use ratatui::{Frame, prelude::Rect};

pub mod production;
pub mod test;

pub use production::ProductionRuntime;
pub use test::{TestRuntime, TestStep};

/// Runtime abstraction for the node launchpad TUI application.
///
/// This trait provides a unified interface for both production and test runtimes,
/// enabling the same application code to run in different execution contexts.
///
/// # Implementation Notes
///
/// - Production implementations should integrate with the terminal I/O
/// - Test implementations should provide event scripting and frame capture
/// - All methods should be designed for async/await compatibility
/// - Error handling should propagate through Result types
///
/// # Examples
///
/// ```rust,no_run
/// use node_launchpad::runtime::{ProductionRuntime, Runtime};
///
/// async fn run_app() -> color_eyre::Result<()> {
///     let mut runtime = ProductionRuntime::new(60.0, 30.0)?;
///     
///     runtime.enter()?;
///     
///     while let Some(event) = runtime.next_event().await {
///         // Handle events...
///     }
///     
///     runtime.exit()?;
///     Ok(())
/// }
/// ```
pub trait Runtime {
    /// Gets the next event from the runtime.
    ///
    /// This method should return `None` when the runtime is shutting down
    /// or when no more events are available.
    #[allow(async_fn_in_trait)]
    async fn next_event(&mut self) -> Option<tui::Event>;

    /// Renders a frame using the provided render function.
    ///
    /// The render function receives a mutable reference to a Frame
    /// and should perform all drawing operations within that closure.
    fn draw(&mut self, render_fn: Box<dyn FnOnce(&mut Frame) + '_>) -> Result<()>;

    /// Initializes the runtime for execution.
    ///
    /// This typically involves setting up terminal modes, clearing screen,
    /// or preparing the execution environment.
    fn enter(&mut self) -> Result<()>;

    /// Shuts down the runtime cleanly.
    ///
    /// This should restore terminal state and clean up any resources.
    fn exit(&mut self) -> Result<()>;

    /// Suspends the runtime temporarily.
    ///
    /// Used when the application needs to temporarily release terminal control.
    fn suspend(&mut self) -> Result<()>;

    /// Stops the runtime execution.
    ///
    /// Different from exit() - this may be called during normal operation
    /// to signal that the runtime should stop processing events.
    fn stop(&mut self) -> Result<()>;

    /// Resizes the runtime to the specified dimensions.
    fn resize(&mut self, rect: Rect) -> Result<()>;

    /// Gets the current size of the runtime.
    fn size(&self) -> Result<Rect>;

    /// Provides access to the concrete runtime type for test-specific operations.
    ///
    /// This method is only available in test builds and should be used sparingly.
    /// It enables test-specific functionality like assertion checking.
    #[cfg(test)]
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any;
}
