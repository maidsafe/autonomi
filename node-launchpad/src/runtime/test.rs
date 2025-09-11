// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! Test runtime implementation for the node launchpad TUI.
//!
//! This module provides test infrastructure for running automated UI tests
//! with event scripting, frame capture, and assertion checking capabilities.

use super::{RenderFn, Runtime};
use crate::{app::App, mode::Scene, tui};
use color_eyre::eyre::Result;
use crossterm::event::KeyEvent;
use ratatui::{Terminal, backend::TestBackend, buffer::Buffer, prelude::Rect};
use std::collections::VecDeque;
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::error;

/// Maximum number of frames to keep in the capture buffer.
const MAX_CAPTURED_FRAMES: usize = 20;

/// A step in a test script that defines an action or assertion.
#[derive(Debug, Clone)]
pub enum TestStep {
    /// Inject a key event into the event stream.
    InjectKey(KeyEvent),
    /// Inject a custom TUI event into the event stream.
    InjectEvent(tui::Event),
    /// Wait for a specified duration (converted to tick events).
    Wait(Duration),
    /// Assert that the application is in a specific scene.
    ExpectScene(Scene),
    /// Assert that the screen contains specific text.
    ExpectText(String),
    /// Assert that the screen matches exactly the given lines.
    ExactScreen(Vec<String>),
    /// Exit the test execution.
    Exit,
}

/// Test runtime that provides event scripting and frame capture for UI testing.
///
/// This runtime captures all rendered frames and can execute scripted test
/// sequences, making it ideal for automated UI testing scenarios.
///
/// # Features
///
/// - Event scripting with TestStep sequences
/// - Frame capture with circular buffer
/// - Test assertions integrated into the event loop
/// - Priority-based event queuing (dynamic > scripted > manual)
pub struct TestRuntime {
    event_receiver: mpsc::UnboundedReceiver<tui::Event>,
    terminal: Terminal<TestBackend>,
    captured_frames: VecDeque<Box<Buffer>>,
    size: Rect,
    test_script: Vec<TestStep>,
    current_step: usize,
    pending_assertion: Option<TestStep>,
    dynamic_event_queue: VecDeque<tui::Event>,
}

impl TestRuntime {
    /// Creates a new test runtime with the specified dimensions.
    ///
    /// # Arguments
    ///
    /// * `event_receiver` - Channel receiver for manual event injection
    /// * `width` - Width of the virtual terminal
    /// * `height` - Height of the virtual terminal
    ///
    /// # Errors
    ///
    /// Returns an error if the test terminal cannot be created.
    pub fn new(
        event_receiver: mpsc::UnboundedReceiver<tui::Event>,
        width: u16,
        height: u16,
    ) -> Result<Self> {
        let backend = TestBackend::new(width, height);
        let terminal = Terminal::new(backend)?;
        let size = Rect::new(0, 0, width, height);

        Ok(Self {
            event_receiver,
            terminal,
            captured_frames: VecDeque::with_capacity(MAX_CAPTURED_FRAMES),
            size,
            test_script: Vec::new(),
            current_step: 0,
            pending_assertion: None,
            dynamic_event_queue: VecDeque::new(),
        })
    }

    /// Creates a new test runtime with default event handling.
    ///
    /// This is a convenience constructor for simple test scenarios that don't
    /// need manual event injection. The runtime will only process scripted
    /// events and dynamically queued events.
    ///
    /// # Arguments
    ///
    /// * `width` - Width of the virtual terminal
    /// * `height` - Height of the virtual terminal
    ///
    /// # Errors
    ///
    /// Returns an error if the test terminal cannot be created.
    pub fn new_simple(width: u16, height: u16) -> Result<Self> {
        let (_sender, receiver) = mpsc::unbounded_channel();
        Self::new(receiver, width, height)
    }

    /// Sets the test script to execute.
    ///
    /// This replaces any existing script and resets execution to the beginning.
    pub fn set_script(&mut self, script: Vec<TestStep>) {
        self.test_script = script;
        self.current_step = 0;
    }

    /// Gets the next event from the test script.
    ///
    /// Returns `Some(Event::Quit)` when the script is complete.
    fn get_next_script_event(&mut self) -> Option<tui::Event> {
        if self.current_step >= self.test_script.len() {
            return Some(tui::Event::Quit); // Exit when script is done
        }

        let step = self.test_script[self.current_step].clone();
        self.current_step += 1;

        match step {
            TestStep::InjectKey(key) => Some(tui::Event::Key(key)),
            TestStep::InjectEvent(event) => Some(event),
            TestStep::Wait(_duration) => {
                // For wait steps, we'll just inject a tick and handle timing elsewhere
                Some(tui::Event::Tick)
            }
            TestStep::ExpectScene(_) | TestStep::ExpectText(_) | TestStep::ExactScreen(_) => {
                // Store assertion for later execution and inject a render to trigger assertion check
                self.pending_assertion = Some(step);
                Some(tui::Event::Render)
            }
            TestStep::Exit => Some(tui::Event::Quit),
        }
    }

    /// Checks and executes any pending test assertion.
    ///
    /// This method should be called after rendering to validate the current
    /// application state against expected test conditions.
    ///
    /// # Arguments
    ///
    /// * `app` - Reference to the application for state inspection
    ///
    /// # Errors
    ///
    /// Returns an error if any assertion fails, containing details about
    /// the expected vs actual state.
    pub fn check_pending_assertion(&mut self, app: &App) -> Result<()> {
        if let Some(assertion) = self.pending_assertion.take() {
            match assertion {
                TestStep::ExpectScene(expected_scene) => {
                    if app.scene != expected_scene {
                        return Err(color_eyre::eyre::eyre!(
                            "Expected scene {:?}, got {:?}",
                            expected_scene,
                            app.scene
                        ));
                    }
                }
                TestStep::ExpectText(text) => {
                    if let Some(buffer) = self.get_last_frame() {
                        let screen_content =
                            crate::test_utils::test_helpers::buffer_to_lines(buffer);
                        let found = screen_content.iter().any(|line| line.contains(&text));
                        if !found {
                            return Err(color_eyre::eyre::eyre!(
                                "Screen does not contain text: '{}'. Actual screen content: {:?}",
                                text,
                                screen_content
                            ));
                        }
                    }
                }
                TestStep::ExactScreen(expected_lines) => {
                    if let Some(buffer) = self.get_last_frame() {
                        let screen_lines = crate::test_utils::test_helpers::buffer_to_lines(buffer);
                        if screen_lines != expected_lines {
                            // Find the first differing line for a helpful error message
                            let mut first_diff_line = None;
                            let max_lines = screen_lines.len().max(expected_lines.len());

                            for i in 0..max_lines {
                                let actual_line = screen_lines
                                    .get(i)
                                    .map(|s| s.as_str())
                                    .unwrap_or("[MISSING]");
                                let expected_line = expected_lines
                                    .get(i)
                                    .map(|s| s.as_str())
                                    .unwrap_or("[MISSING]");

                                if actual_line != expected_line {
                                    first_diff_line = Some((i + 1, actual_line, expected_line));
                                    break;
                                }
                            }

                            let error_msg = if let Some((line_num, actual, expected)) =
                                first_diff_line
                            {
                                format!(
                                    "Screen content mismatch at line {line_num}:\n  Expected: '{expected}'\n  Actual:   '{actual}'"
                                )
                            } else {
                                format!(
                                    "Screen content mismatch. Expected {} lines, got {} lines",
                                    expected_lines.len(),
                                    screen_lines.len()
                                )
                            };

                            return Err(color_eyre::eyre::eyre!("{}", error_msg));
                        }
                    } else {
                        return Err(color_eyre::eyre::eyre!(
                            "No frame was captured for screen assertion"
                        ));
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Gets references to all captured frames.
    pub fn get_captured_frames(&self) -> Vec<&Buffer> {
        self.captured_frames.iter().map(|b| b.as_ref()).collect()
    }

    /// Gets a reference to the most recently captured frame.
    pub fn get_last_frame(&self) -> Option<&Buffer> {
        self.captured_frames.back().map(|b| b.as_ref())
    }

    /// Clears all captured frames from the buffer.
    pub fn clear_frames(&mut self) {
        self.captured_frames.clear();
    }

    /// Queues an event for immediate processing.
    ///
    /// Events queued this way take priority over scripted events.
    pub fn queue_event(&mut self, event: tui::Event) {
        self.dynamic_event_queue.push_back(event);
    }
}

impl Runtime for TestRuntime {
    async fn next_event(&mut self) -> Option<tui::Event> {
        // Priority 1: Check dynamic event queue first (for helper method events)
        if let Some(event) = self.dynamic_event_queue.pop_front() {
            return Some(event);
        }

        // Priority 2: Fall back to scripted events (for Journey steps)
        if !self.test_script.is_empty()
            && let Some(event) = self.get_next_script_event()
        {
            return Some(event);
        }

        // Priority 3: Fall back to manual event injection (for backward compatibility)
        self.event_receiver.recv().await
    }

    fn draw(&mut self, render_fn: RenderFn<'_>) -> Result<()> {
        let mut result = Ok(());
        self.terminal.draw(|frame| {
            if let Err(e) = render_fn(frame) {
                result = Err(e);
            }
        })?;

        // Safer buffer handling to prevent Windows memory issues
        let buffer = match std::panic::catch_unwind(|| self.terminal.backend().buffer().clone()) {
            Ok(buf) => buf,
            Err(_) => {
                error!("Buffer clone failed, skipping frame capture");
                println!("Buffer clone failed, skipping frame capture");
                return result;
            }
        };

        // Implement circular buffer with bounds checking
        while self.captured_frames.len() >= MAX_CAPTURED_FRAMES {
            if self.captured_frames.pop_front().is_none() {
                error!("Failed to remove oldest frame from buffer");
                println!("Failed to remove oldest frame from buffer");
                break;
            }
        }

        // Box the buffer to reduce stack pressure
        self.captured_frames.push_back(Box::new(buffer));
        result
    }

    fn enter(&mut self) -> Result<()> {
        Ok(())
    }

    fn exit(&mut self) -> Result<()> {
        Ok(())
    }

    fn suspend(&mut self) -> Result<()> {
        Ok(())
    }

    fn stop(&mut self) -> Result<()> {
        Ok(())
    }

    fn resize(&mut self, rect: Rect) -> Result<()> {
        self.size = rect;
        let backend = TestBackend::new(rect.width, rect.height);
        self.terminal = Terminal::new(backend)?;
        Ok(())
    }

    fn size(&self) -> Result<Rect> {
        Ok(self.size)
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}
