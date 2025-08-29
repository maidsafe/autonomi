// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use super::{
    keyboard::KeySequence,
    test_helpers::{TestAppBuilder, buffer_to_lines},
};
use crate::{
    action::Action,
    app::App,
    focus::FocusTarget,
    mode::{InputMode, Scene},
};
use color_eyre::{Result, eyre::eyre};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend, buffer::Buffer};
use std::time::{Duration, Instant};
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};

pub struct Journey {
    pub name: String,
    pub steps: Vec<JourneyStep>,
    pub app: App,
    action_tx: UnboundedSender<Action>,
    action_rx: UnboundedReceiver<Action>,
}

#[derive(Debug, Clone)]
pub struct JourneyStep {
    pub keys: Vec<KeyEvent>,
    pub expected_scene: Option<Scene>,
    pub assertions: Vec<ScreenAssertion>,
}

#[derive(Debug, Clone)]
pub enum ScreenAssertion {
    ExactScreen(Vec<String>),
    ContainsText(String),
    SceneIs(Scene),
}

impl Journey {
    pub fn new(name: String, mut app: App) -> Self {
        let (action_tx, action_rx) = mpsc::unbounded_channel();

        // Initialize components with action handler
        for component in app.components.iter_mut() {
            component.register_action_handler(action_tx.clone()).ok();
        }

        Self {
            name,
            steps: Vec::new(),
            app,
            action_tx,
            action_rx,
        }
    }

    pub fn add_step(&mut self, step: JourneyStep) {
        self.steps.push(step);
    }

    pub async fn run(&mut self) -> Result<()> {
        for (step_index, step) in self.steps.clone().into_iter().enumerate() {
            println!("Journey '{}': Executing step {}", self.name, step_index + 1);

            self.process_keys(&step.keys).await?;
            tokio::time::sleep(Duration::from_millis(50)).await;

            if let Some(expected_scene) = &step.expected_scene
                && self.app.scene != *expected_scene
            {
                return Err(eyre!(
                    "Journey '{}' step {}: Expected scene {:?}, got {:?}",
                    self.name,
                    step_index + 1,
                    expected_scene,
                    self.app.scene
                ));
            }

            for assertion in &step.assertions {
                self.assert_current_screen(assertion).await?;
            }
        }

        println!("Journey '{}': Completed successfully", self.name);
        Ok(())
    }

    pub fn render_screen(&mut self) -> Result<Buffer> {
        let mut terminal = Terminal::new(TestBackend::new(160, 30))?;

        terminal.draw(|f| {
            for component in self.app.components.iter_mut() {
                let should_draw = self
                    .app
                    .focus_manager
                    .get_focus_stack()
                    .contains(&component.focus_target());

                if should_draw && let Err(e) = component.draw(f, f.area()) {
                    eprintln!("Failed to draw component: {e}");
                }
            }
        })?;

        Ok(terminal.backend().buffer().clone())
    }

    pub async fn assert_current_screen(&mut self, assertion: &ScreenAssertion) -> Result<()> {
        let buffer = self.render_screen()?;
        let screen_lines = buffer_to_lines(&buffer);

        match assertion {
            ScreenAssertion::ExactScreen(expected_lines) => {
                if screen_lines.len() != expected_lines.len() {
                    return Err(eyre!(
                        "Screen has {} lines, expected {}",
                        screen_lines.len(),
                        expected_lines.len()
                    ));
                }

                for (i, (actual, expected)) in
                    screen_lines.iter().zip(expected_lines.iter()).enumerate()
                {
                    if actual != expected {
                        return Err(eyre!(
                            "Line {} mismatch:\n  Actual:   '{}'\n  Expected: '{}'",
                            i + 1,
                            actual,
                            expected
                        ));
                    }
                }
            }
            ScreenAssertion::ContainsText(text) => {
                let found = screen_lines.iter().any(|line| line.contains(text));
                if !found {
                    return Err(eyre!("Screen does not contain text: '{}'", text));
                }
            }
            ScreenAssertion::SceneIs(expected_scene) => {
                if self.app.scene != *expected_scene {
                    return Err(eyre!(
                        "Expected scene {:?}, got {:?}",
                        expected_scene,
                        self.app.scene
                    ));
                }
            }
        }

        Ok(())
    }

    pub async fn process_keys(&mut self, keys: &[KeyEvent]) -> Result<Vec<Action>> {
        let mut all_actions = Vec::new();

        for key in keys {
            // Use the real app's key handling logic
            let key_actions = self.app.handle_key_event(*key, &self.action_tx).await?;
            all_actions.extend(key_actions);

            // Process the action queue after each key
            let processed_actions = self.process_action_queue().await?;
            all_actions.extend(processed_actions);
        }

        Ok(all_actions)
    }

    async fn process_action_queue(&mut self) -> Result<Vec<Action>> {
        let mut processed_actions = Vec::new();

        while let Ok(action) = self.action_rx.try_recv() {
            // Use the real app's action processing logic
            let generated_actions = self.app.process_action(action.clone(), &self.action_tx)?;
            processed_actions.push(action);
            processed_actions.extend(generated_actions);
        }

        Ok(processed_actions)
    }

    pub fn switch_to_input_mode(&mut self) {
        self.app.input_mode = InputMode::Entry;
    }

    pub fn switch_to_navigation_mode(&mut self) {
        self.app.input_mode = InputMode::Navigation;
    }

    pub fn focus_component(&mut self, target: FocusTarget) {
        self.app.focus_manager.set_focus(target);
    }

    pub async fn enter_text(&mut self, text: &str) -> Result<Vec<Action>> {
        let mut actions = Vec::new();

        // Switch to input mode if not already
        if self.app.input_mode != InputMode::Entry {
            self.switch_to_input_mode();
        }

        // Type each character
        for c in text.chars() {
            let key = KeyEvent::new(KeyCode::Char(c), KeyModifiers::empty());
            let key_actions = self.process_keys(&[key]).await?;
            actions.extend(key_actions);
        }

        Ok(actions)
    }

    pub async fn press_and_process(&mut self, key: KeyEvent) -> Result<Vec<Action>> {
        self.process_keys(&[key]).await
    }

    pub async fn wait_for_scene(&mut self, scene: Scene, timeout_ms: u64) -> Result<()> {
        if self.app.scene != scene {
            let start = Instant::now();
            let timeout_duration = Duration::from_millis(timeout_ms);

            loop {
                if self.app.scene == scene {
                    break;
                }
                if start.elapsed() > timeout_duration {
                    return Err(eyre!(
                        "Timeout waiting for scene {:?}, current scene is {:?}",
                        scene,
                        self.app.scene
                    ));
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }

        Ok(())
    }
}

pub struct JourneyBuilder {
    journey: Journey,
    current_step: Option<JourneyStep>,
}

impl JourneyBuilder {
    pub async fn new(name: &str) -> Result<Self> {
        let app = TestAppBuilder::new().build().await?;

        Ok(Self {
            journey: Journey::new(name.to_string(), app),
            current_step: None,
        })
    }

    pub async fn new_with_nodes(name: &str, node_count: u64) -> Result<Self> {
        let app = TestAppBuilder::new().with_nodes(node_count).build().await?;

        Ok(Self {
            journey: Journey::new(name.to_string(), app),
            current_step: None,
        })
    }

    pub fn start_from(mut self, scene: Scene) -> Self {
        self.journey.app.scene = scene;
        self
    }

    pub fn press(mut self, keys: impl Into<KeySequence>) -> Self {
        let key_sequence: KeySequence = keys.into();
        let key_events = key_sequence.build();

        if let Some(ref mut step) = self.current_step {
            step.keys.extend(key_events);
        } else {
            self.current_step = Some(JourneyStep {
                keys: key_events,
                expected_scene: None,
                assertions: Vec::new(),
            });
        }
        self
    }

    pub fn press_key(mut self, key: KeyEvent) -> Self {
        if let Some(ref mut step) = self.current_step {
            step.keys.push(key);
        } else {
            self.current_step = Some(JourneyStep {
                keys: vec![key],
                expected_scene: None,
                assertions: Vec::new(),
            });
        }
        self
    }

    pub fn expect_scene(mut self, scene: Scene) -> Self {
        if let Some(ref mut step) = self.current_step {
            step.expected_scene = Some(scene);
        } else {
            self.current_step = Some(JourneyStep {
                keys: Vec::new(),
                expected_scene: Some(scene),
                assertions: Vec::new(),
            });
        }
        self
    }

    pub fn expect_text(mut self, text: &str) -> Self {
        if let Some(ref mut step) = self.current_step {
            step.assertions
                .push(ScreenAssertion::ContainsText(text.to_string()));
        } else {
            self.current_step = Some(JourneyStep {
                keys: Vec::new(),
                expected_scene: None,
                assertions: vec![ScreenAssertion::ContainsText(text.to_string())],
            });
        }
        self
    }

    pub fn expect_screen(mut self, screen: &[&str]) -> Self {
        let screen_lines: Vec<String> = screen.iter().map(|s| s.to_string()).collect();

        if let Some(ref mut step) = self.current_step {
            step.assertions
                .push(ScreenAssertion::ExactScreen(screen_lines));
        } else {
            self.current_step = Some(JourneyStep {
                keys: Vec::new(),
                expected_scene: None,
                assertions: vec![ScreenAssertion::ExactScreen(screen_lines)],
            });
        }
        self
    }

    pub fn step(mut self) -> Self {
        if let Some(step) = self.current_step.take() {
            self.journey.add_step(step);
        }
        self
    }

    pub async fn run(mut self) -> Result<()> {
        if let Some(step) = self.current_step.take() {
            self.journey.add_step(step);
        }

        self.journey.run().await
    }

    pub async fn build(mut self) -> Result<Journey> {
        if let Some(step) = self.current_step.take() {
            self.journey.add_step(step);
        }

        Ok(self.journey)
    }
}

impl From<char> for KeySequence {
    fn from(c: char) -> Self {
        KeySequence::new().key(c)
    }
}

impl From<KeyEvent> for KeySequence {
    fn from(key: KeyEvent) -> Self {
        KeySequence::new().push_event(key)
    }
}

impl From<&str> for KeySequence {
    fn from(s: &str) -> Self {
        KeySequence::new().string(s)
    }
}

impl From<Vec<KeyEvent>> for KeySequence {
    fn from(events: Vec<KeyEvent>) -> Self {
        let mut seq = KeySequence::new();
        for event in events {
            seq = seq.push_event(event);
        }
        seq
    }
}
