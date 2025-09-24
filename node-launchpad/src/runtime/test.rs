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
use color_eyre::eyre::{Result, eyre};
use crossterm::event::KeyEvent;
use ratatui::{Terminal, backend::TestBackend, buffer::Buffer, prelude::Rect};
use std::collections::VecDeque;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

type AppResultPredicate = Arc<dyn Fn(&App) -> Result<()> + Send + Sync>;
type AppBoolPredicate = Arc<dyn Fn(&App) -> Result<bool> + Send + Sync>;
use tokio::sync::mpsc;
use tokio::time::Instant;
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
    /// Advance Tokio's virtual time by a duration.
    AdvanceTime(Duration),
    /// Wait for a predicate to succeed (polling until timeout).
    WaitForCondition(WaitCondition),
    /// Assert that the application is in a specific scene.
    ExpectScene(Scene),
    /// Assert that the screen contains specific text.
    ExpectText(String),
    /// Assert that the screen matches exactly the given lines.
    ExactScreen(Vec<String>),
    /// Assert arbitrary state by running a predicate against the app.
    AssertState(StateAssertion),
    /// Exit the test execution.
    Exit,
}

#[derive(Clone)]
pub struct StateAssertion {
    description: String,
    predicate: AppResultPredicate,
}

impl StateAssertion {
    pub fn new(description: impl Into<String>, predicate: AppResultPredicate) -> Self {
        Self {
            description: description.into(),
            predicate,
        }
    }

    pub fn evaluate(&self, app: &App) -> Result<()> {
        (self.predicate)(app)
    }

    pub fn description(&self) -> &str {
        &self.description
    }
}

impl fmt::Debug for StateAssertion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StateAssertion")
            .field("description", &self.description)
            .finish()
    }
}

#[derive(Clone)]
pub struct WaitCondition {
    description: String,
    predicate: AppBoolPredicate,
    timeout: Duration,
    poll_interval: Duration,
}

impl WaitCondition {
    pub fn new(
        description: impl Into<String>,
        predicate: AppBoolPredicate,
        timeout: Duration,
        poll_interval: Duration,
    ) -> Self {
        Self {
            description: description.into(),
            predicate,
            timeout,
            poll_interval,
        }
    }

    pub fn evaluate(&self, app: &App) -> Result<bool> {
        (self.predicate)(app)
    }

    pub fn timeout(&self) -> Duration {
        self.timeout
    }

    pub fn poll_interval(&self) -> Duration {
        self.poll_interval
    }

    pub fn description(&self) -> &str {
        &self.description
    }
}

impl fmt::Debug for WaitCondition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WaitCondition")
            .field("description", &self.description)
            .field("timeout", &self.timeout)
            .field("poll_interval", &self.poll_interval)
            .finish()
    }
}

struct PendingWait {
    condition: WaitCondition,
    deadline: Instant,
    next_poll: Instant,
}

enum PendingAssertion {
    Scene(Scene),
    Text(String),
    ExactScreen(Vec<String>),
    State(StateAssertion),
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
    pending_assertion: Option<PendingAssertion>,
    pending_wait: Option<PendingWait>,
    pending_time_advances: VecDeque<Duration>,
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
            pending_wait: None,
            pending_time_advances: VecDeque::new(),
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
            TestStep::Wait(duration) => {
                // For wait steps, convert to a tick; actual timing handled in next_event
                self.pending_time_advances.push_back(duration);
                Some(tui::Event::Tick)
            }
            TestStep::AdvanceTime(duration) => {
                self.pending_time_advances.push_back(duration);
                Some(tui::Event::Tick)
            }
            TestStep::WaitForCondition(condition) => {
                let now = Instant::now();
                let condition_clone = condition.clone();
                let timeout = condition_clone.timeout();
                self.pending_wait = Some(PendingWait {
                    condition: condition_clone,
                    deadline: now + timeout,
                    next_poll: now,
                });
                Some(tui::Event::Render)
            }
            TestStep::ExpectScene(expected_scene) => {
                self.pending_assertion = Some(PendingAssertion::Scene(expected_scene));
                Some(tui::Event::Render)
            }
            TestStep::ExpectText(text) => {
                self.pending_assertion = Some(PendingAssertion::Text(text));
                Some(tui::Event::Render)
            }
            TestStep::ExactScreen(lines) => {
                self.pending_assertion = Some(PendingAssertion::ExactScreen(lines));
                Some(tui::Event::Render)
            }
            TestStep::AssertState(assertion) => {
                self.pending_assertion = Some(PendingAssertion::State(assertion));
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
                PendingAssertion::Scene(expected_scene) => {
                    if app.scene != expected_scene {
                        return Err(eyre!(
                            "Expected scene {:?}, got {:?}",
                            expected_scene,
                            app.scene
                        ));
                    }
                }
                PendingAssertion::Text(text) => {
                    if let Some(buffer) = self.get_last_frame() {
                        let screen_content =
                            crate::test_utils::test_helpers::buffer_to_lines(buffer);
                        let found = screen_content.iter().any(|line| line.contains(&text));
                        if !found {
                            return Err(eyre!(
                                "Screen does not contain text: '{}'. Actual screen content: {:?}",
                                text,
                                screen_content
                            ));
                        }
                    } else {
                        return Err(eyre!("No frame captured while checking for text"));
                    }
                }
                PendingAssertion::ExactScreen(expected_lines) => {
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

                            return Err(eyre!(error_msg));
                        }
                    } else {
                        return Err(eyre!("No frame was captured for screen assertion"));
                    }
                }
                PendingAssertion::State(assertion) => {
                    assertion.evaluate(app)?;
                }
            }
        }

        self.process_pending_wait(app)
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

    fn process_pending_wait(&mut self, app: &App) -> Result<()> {
        if let Some(mut wait) = self.pending_wait.take() {
            let now = Instant::now();

            if now >= wait.deadline {
                return Err(eyre!(
                    "Timed out waiting for condition: {}",
                    wait.condition.description()
                ));
            }

            if wait.condition.evaluate(app)? {
                // Condition satisfied, nothing else to do.
                return Ok(());
            }

            // Condition not yet satisfied; schedule another render after the poll interval.
            let next_poll = now + wait.condition.poll_interval();
            wait.next_poll = next_poll;
            self.pending_wait = Some(wait);
            let delay = next_poll.saturating_duration_since(now);
            self.pending_time_advances.push_back(delay);
            self.dynamic_event_queue.push_back(tui::Event::Render);
        }

        Ok(())
    }
}

impl Runtime for TestRuntime {
    async fn next_event(&mut self) -> Option<tui::Event> {
        while let Some(delay) = self.pending_time_advances.pop_front() {
            if !delay.is_zero() {
                tokio::time::sleep(delay).await;
            }
            // Multiple queued delays should be processed sequentially before sourcing next event.
        }

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
