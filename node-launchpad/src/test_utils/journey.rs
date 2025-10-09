// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use super::{
    keyboard::KeySequence,
    mock_metrics::MockMetricsService,
    mock_node_management::{
        MockNodeManagement, MockNodeManagementHandle, MockNodeResponsePlan, ScriptedNodeAction,
    },
    test_helpers::{TestAppBuilder, TestAppContext},
};
use crate::{
    action::{Action, NodeManagementCommand},
    app::App,
    components::{
        node_table::{lifecycle::LifecycleState, view::NodeViewModel},
        status::Status,
    },
    mode::Scene,
    node_stats::AggregatedNodeStats,
    runtime::{Runtime, StateAssertion, TestRuntime, TestStep, WaitCondition},
    tui,
};
use ant_service_management::{ReachabilityProgress, metric::ReachabilityStatusValues};
use color_eyre::{Result, eyre::eyre};
use crossterm::event::KeyEvent;
use std::{sync::Arc, time::Duration};
use tempfile::TempDir;
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio::task::JoinHandle;
use tracing::{debug, error, info};

/// Captures a scripted test run of the TUI application.
/// Construct via `JourneyBuilder` and drive it with scripted key input and expectations.
pub struct Journey {
    pub name: String,
    pub steps: Vec<JourneyStep>,
    app: Option<App>,
    test_runtime: TestRuntime,
    node_management_handle: Option<MockNodeManagementHandle>,
    scripted_tasks: Vec<JoinHandle<()>>,
    mock_node_management: Option<Arc<MockNodeManagement>>,
    registry_dir: Option<TempDir>,
    action_tx: Option<UnboundedSender<Action>>,
    action_rx: Option<UnboundedReceiver<Action>>,
    viewport: ratatui::prelude::Rect,
}

/// Locate the immutable status component for inspection.
/// Pair with `status_component_mut` when you need to stage follow-up mutations.
pub fn status_component(app: &App) -> Result<&Status> {
    app.components
        .iter()
        .find_map(|component| component.as_ref().as_any().downcast_ref::<Status>())
        .ok_or_else(|| eyre!("Status component not found"))
}

/// Locate the mutable status component so tests can tweak it before running journeys.
/// Commonly paired with `TestAppBuilder` to override disk space or inject alternate metrics fetchers.
pub fn status_component_mut(app: &mut App) -> Result<&mut Status> {
    app.components
        .iter_mut()
        .find_map(|component| component.as_mut().as_any_mut().downcast_mut::<Status>())
        .ok_or_else(|| eyre!("Status component not found"))
}

/// Fetch a node view model by service name for assertion helpers.
/// Works in tandem with `expect_node_state`, `assert_spinner`, and other per-node checks.
pub fn node_view_model<'a>(app: &'a App, node_id: &str) -> Result<&'a NodeViewModel> {
    status_component(app)?
        .node_table()
        .view_items()
        .iter()
        .find(|model| model.id == node_id)
        .ok_or_else(|| eyre!("Node `{node_id}` not found in view"))
}

/// High-level step describing key input, expectations, and follow-up actions.
/// Usually produced indirectly via the fluent `JourneyBuilder` API.
#[derive(Debug, Clone)]
pub struct JourneyStep {
    pub keys: Vec<KeyEvent>,
    pub expected_scene: Option<Scene>,
    pub assertions: Vec<ScreenAssertion>,
    pub follow_up_steps: Vec<TestStep>,
}

/// Assertions that act on the rendered screen buffer.
/// Combined inside a `JourneyStep` when composing multi-assertion scenarios.
#[derive(Debug, Clone)]
pub enum ScreenAssertion {
    ExactScreen(Vec<String>),
    ContainsText(String),
}

impl Journey {
    /// Construct a journey from an `App` plus optional mock handle.
    /// Typically invoked by `JourneyBuilder::from_context`.
    pub fn new(
        name: String,
        mut app: App,
        node_management_handle: Option<MockNodeManagementHandle>,
    ) -> Result<Self> {
        let test_runtime = TestRuntime::new_simple(160, 30)?;

        let (action_tx, action_rx) = mpsc::unbounded_channel();
        let rect = ratatui::prelude::Rect::new(0, 0, 160, 30);

        // Ensure components are fully initialized before sending any actions
        app.init_components(rect, action_tx.clone())?;

        // Queue an initial tick so the runtime loop processes startup tasks once it begins
        action_tx.send(Action::Tick)?;

        let journey_app = Some(app);

        info!(journey_name = %name, "Initialised test journey context");

        Ok(Self {
            name,
            steps: Vec::new(),
            app: journey_app,
            test_runtime,
            node_management_handle,
            scripted_tasks: Vec::new(),
            mock_node_management: None,
            registry_dir: None,
            action_tx: Some(action_tx),
            action_rx: Some(action_rx),
            viewport: rect,
        })
    }

    /// Expose the mock node-management handle for custom scripting.
    /// Ideal when a test needs to enqueue bespoke responses instead of relying on `JourneyBuilder`.
    pub fn node_management_handle(&mut self) -> Option<&mut MockNodeManagementHandle> {
        self.node_management_handle.as_mut()
    }

    /// Detach the node-management handle from the journey.
    /// Useful when handing control to background tasks or manual mock coordination.
    pub fn take_node_management_handle(&mut self) -> Option<MockNodeManagementHandle> {
        self.node_management_handle.take()
    }

    /// Track an async task spawned as part of the scripted journey.
    /// Pair with `MockNodeResponsePlan::then_*` helpers when chaining additional async behaviour.
    pub fn register_script_task(&mut self, handle: JoinHandle<()>) {
        self.scripted_tasks.push(handle);
    }

    /// Record a fully-constructed step that is ready to execute.
    /// Pair with builder helpers such as `press` and `expect_text` before calling `run`.
    pub fn add_step(&mut self, step: JourneyStep) {
        self.steps.push(step);
    }

    /// Execute the scripted journey against the test runtime.
    /// Call once all steps have been staged via `JourneyBuilder` and `build`.
    pub async fn run(&mut self) -> Result<()> {
        info!(journey = %self.name, steps = self.steps.len(), "Starting scripted journey run");

        // Convert journey steps to test script
        let script = self.build_test_script();
        debug!(journey = %self.name, actions = script.len(), "Loaded scripted runtime actions");
        self.test_runtime.set_script(script);

        // Actually run the app with the scripted test runtime!
        // This is the key: we use the SAME App::run_with_runtime as production
        if let Some(mut app) = self.app.take() {
            let runtime = &mut self.test_runtime;
            let action_tx = self
                .action_tx
                .take()
                .ok_or_else(|| eyre!("missing action sender for journey run"))?;
            let mut action_rx = self
                .action_rx
                .take()
                .ok_or_else(|| eyre!("missing action receiver for journey run"))?;

            runtime.enter()?;

            // Journey initialisation currently assumes a fixed viewport; ensure the runtime matches.
            debug_assert_eq!(runtime.size()?, self.viewport);

            loop {
                if let Some(event) = runtime.next_event().await {
                    if !matches!(&event, tui::Event::Tick) {
                        debug!(journey = %self.name, ?event, "Runtime emitted event");
                    }

                    match event {
                        tui::Event::Quit => {
                            action_tx.send(Action::Quit)?;
                        }
                        tui::Event::Tick => action_tx.send(Action::Tick)?,
                        tui::Event::Render => action_tx.send(Action::Render)?,
                        tui::Event::Resize(w, h) => action_tx.send(Action::Resize(w, h))?,
                        tui::Event::Key(key) => {
                            app.handle_key_event(key, &action_tx)?;
                        }
                        _ => {}
                    }
                }

                while let Ok(action) = action_rx.try_recv() {
                    if !matches!(&action, Action::Tick | Action::Render) {
                        debug!(journey = %self.name, ?action, "Dequeued action");
                    }

                    match action {
                        Action::Resize(w, h) => {
                            runtime.resize(ratatui::prelude::Rect::new(0, 0, w, h))?;
                            runtime.draw(Box::new(|f| app.render_frame(f, &action_tx)))?;
                        }
                        Action::Render => {
                            runtime.draw(Box::new(|f| app.render_frame(f, &action_tx)))?;
                            runtime.check_pending_assertion(&app)?;
                        }
                        _ => {
                            app.process_action(action.clone(), &action_tx)?;
                        }
                    }
                }

                if app.should_suspend {
                    runtime.suspend()?;
                    action_tx.send(Action::Resume)?;
                    runtime.enter()?;
                } else if app.should_quit {
                    debug!(journey = %self.name, "Processing pending actions before quit");
                    let mut pending_actions = Vec::new();
                    while let Ok(action) = action_rx.try_recv() {
                        pending_actions.push(action);
                    }

                    for action in pending_actions {
                        if action != Action::Tick {
                            debug!(journey = %self.name, ?action, "Processing final action before quit");
                        }
                        match action {
                            Action::Render => {
                                runtime.draw(Box::new(|f| app.render_frame(f, &action_tx)))?;
                                runtime.check_pending_assertion(&app)?;
                            }
                            _ => {
                                app.process_action(action, &action_tx)?;
                            }
                        }
                    }

                    runtime.stop()?;
                    break;
                }
            }

            runtime.exit()?;
        }

        // Drop mocks to close any channels before awaiting scripted tasks
        self.node_management_handle.take();
        self.mock_node_management.take();

        for task in self.scripted_tasks.drain(..) {
            let _ = task.await;
        }

        info!(journey = %self.name, "Completed scripted journey run");

        Ok(())
    }

    /// Materialise the queued steps into runtime-friendly test steps.
    /// Prefer calling `run` directly unless you need to inject the script into a custom runtime.
    fn build_test_script(&self) -> Vec<crate::runtime::TestStep> {
        let mut script = Vec::new();

        for (index, step) in self.steps.iter().enumerate() {
            debug!(
                journey = %self.name,
                step_index = index,
                key_events = step.keys.len(),
                assertions = step.assertions.len(),
                follow_ups = step.follow_up_steps.len(),
                has_scene_expectation = step.expected_scene.is_some(),
                "Translating scripted journey step"
            );

            // Add key events
            for key in &step.keys {
                script.push(TestStep::InjectKey(*key));
            }

            // Add scene expectation if present
            if let Some(expected_scene) = step.expected_scene {
                script.push(TestStep::ExpectScene(expected_scene));
            }

            // Add screen assertions
            for assertion in &step.assertions {
                match assertion {
                    ScreenAssertion::ExactScreen(lines) => {
                        script.push(TestStep::ExactScreen(lines.clone()));
                    }
                    ScreenAssertion::ContainsText(text) => {
                        script.push(TestStep::ExpectText(text.clone()));
                    }
                }
            }

            for follow_up in &step.follow_up_steps {
                script.push(follow_up.clone());
            }

            // Add a small wait between steps
            script.push(TestStep::Wait(Duration::from_millis(50)));
        }

        // Add exit at the end to terminate the test properly
        script.push(TestStep::Exit);

        script
    }
}

pub struct JourneyBuilder {
    journey: Journey,
    current_step: Option<JourneyStep>,
    node_action_scripts: Vec<ScriptedNodeAction>,
}

impl JourneyBuilder {
    /// Create a journey with a fresh app containing no nodes.
    /// Chain `.start_from` or `.press` calls to define behaviour before `.run`.
    pub async fn new(name: &str) -> Result<Self> {
        Self::new_with_setup(name, |builder| builder).await
    }

    /// Create a journey with a pre-seeded set of running nodes.
    /// Add scripted responses via `.with_node_action_response` for lifecycle transitions.
    pub async fn new_with_nodes(name: &str, node_count: u64) -> Result<Self> {
        Self::new_with_setup(name, move |builder| builder.with_running_nodes(node_count)).await
    }

    /// Create a journey with a fully custom `TestAppBuilder` configuration.
    /// Useful when combining multiple builder customisations before testing.
    pub async fn new_with_setup<F>(name: &str, setup: F) -> Result<Self>
    where
        F: FnOnce(TestAppBuilder) -> TestAppBuilder,
    {
        let builder = setup(TestAppBuilder::new());
        Self::from_context(name, builder.build().await?)
    }

    /// Construct a builder from an already-built `TestAppContext`.
    /// Handy when the test needs to manipulate the raw app before scripting steps.
    pub fn from_context(name: &str, context: TestAppContext) -> Result<Self> {
        let TestAppContext {
            app,
            node_management_handle,
            mock_node_management,
            registry_dir,
            ..
        } = context;

        debug!(
            journey_name = name,
            "Creating journey builder from test context"
        );

        Ok(Self {
            journey: {
                let mut journey = Journey::new(name.to_string(), app, node_management_handle)?;
                journey.mock_node_management = mock_node_management;
                journey.registry_dir = registry_dir;
                journey
            },
            current_step: None,
            node_action_scripts: Vec::new(),
        })
    }

    /// Override the metrics script injected into the status component.
    /// Pair with `with_node_action_response` to ensure metrics align with scripted events.
    pub fn with_metrics_script(mut self, script: Vec<AggregatedNodeStats>) -> Self {
        if let Some(app) = self.journey.app.as_mut() {
            match status_component_mut(app) {
                Ok(status) => {
                    let script_len = script.len();
                    let fetcher = MockMetricsService::scripted(script);
                    status
                        .node_table_mut()
                        .state_mut()
                        .set_metrics_fetcher(fetcher);
                    debug!(
                        journey = %self.journey.name,
                        metrics_samples = script_len,
                        "Configured scripted metrics fetcher"
                    );
                }
                Err(err) => {
                    error!("Failed to configure metrics script: {err}");
                }
            }
        }
        self
    }

    /// Register a scripted response for a node-management command.
    /// Best combined with `MockNodeResponsePlan` chaining helpers like `then_metrics`.
    pub fn with_node_action_response(
        mut self,
        command: NodeManagementCommand,
        plan: MockNodeResponsePlan,
    ) -> Self {
        let delay_ms = plan.delay.as_millis();
        let follow_ups = plan.followup_events.len();
        let has_response = plan.response.is_some();
        debug!(
            ?command,
            delay_ms, follow_ups, has_response, "Registered scripted node-management response"
        );
        self.node_action_scripts
            .push(ScriptedNodeAction { command, plan });
        self
    }

    /// Start the journey from a non-default scene.
    /// Often followed by `.press` to navigate elsewhere in the UI.
    pub fn start_from(mut self, scene: Scene) -> Self {
        if let Some(app) = self.journey.app.as_mut() {
            app.scene = scene;
        }
        debug!(journey = %self.journey.name, ?scene, "Staged journey start scene");
        self
    }

    /// Queue a sequence of key presses.
    /// Combine with `.expect_scene`/`.expect_text` to validate resultant state.
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
                follow_up_steps: Vec::new(),
            });
        }
        self
    }

    /// Queue a single key event.
    /// Useful between `.step()` calls for fine-grained navigation.
    pub fn press_key(mut self, key: KeyEvent) -> Self {
        if let Some(ref mut step) = self.current_step {
            step.keys.push(key);
        } else {
            self.current_step = Some(JourneyStep {
                keys: vec![key],
                expected_scene: None,
                assertions: Vec::new(),
                follow_up_steps: Vec::new(),
            });
        }
        self
    }

    /// Assert that the app enters the specified scene.
    /// Typically paired with `.press` or `.press_key` to trigger the transition.
    pub fn expect_scene(mut self, scene: Scene) -> Self {
        if let Some(ref mut step) = self.current_step {
            step.expected_scene = Some(scene);
        } else {
            self.current_step = Some(JourneyStep {
                keys: Vec::new(),
                expected_scene: Some(scene),
                assertions: Vec::new(),
                follow_up_steps: Vec::new(),
            });
        }
        self
    }

    /// Assert that the rendered buffer contains the provided snippet.
    /// Combine with `.press` or `.wait` to assert dynamic content updates.
    pub fn expect_text(mut self, text: &str) -> Self {
        if let Some(ref mut step) = self.current_step {
            step.assertions
                .push(ScreenAssertion::ContainsText(text.to_string()));
        } else {
            self.current_step = Some(JourneyStep {
                keys: Vec::new(),
                expected_scene: None,
                assertions: vec![ScreenAssertion::ContainsText(text.to_string())],
                follow_up_steps: Vec::new(),
            });
        }
        self
    }

    /// Alias for `expect_text` to match tests that use an assertion phrasing.
    pub fn assert_text(self, text: &str) -> Self {
        self.expect_text(text)
    }

    /// Assert that the entire buffer matches the provided reference screen.
    /// Precede with `.wait` to allow rendering to settle before capturing output.
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
                follow_up_steps: Vec::new(),
            });
        }
        self
    }

    /// Insert a delay between scripted actions.
    /// Use alongside `.with_node_action_response` when asynchronous updates are expected.
    pub fn wait(mut self, duration: Duration) -> Self {
        if let Some(ref mut step) = self.current_step {
            step.follow_up_steps.push(TestStep::Wait(duration));
        } else {
            self.current_step = Some(JourneyStep {
                keys: Vec::new(),
                expected_scene: None,
                assertions: Vec::new(),
                follow_up_steps: vec![TestStep::Wait(duration)],
            });
        }
        self
    }

    /// Advance the virtual clock used by the test runtime.
    /// Often coupled with `.wait_for_condition` in time-sensitive flows.
    pub fn advance_time(mut self, duration: Duration) -> Self {
        if let Some(ref mut step) = self.current_step {
            step.follow_up_steps.push(TestStep::AdvanceTime(duration));
        } else {
            self.current_step = Some(JourneyStep {
                keys: Vec::new(),
                expected_scene: None,
                assertions: Vec::new(),
                follow_up_steps: vec![TestStep::AdvanceTime(duration)],
            });
        }
        self
    }

    /// Poll the app until the supplied predicate returns `true` or times out.
    /// Works well with `.wait_for_node_state`/`.wait_for_reachability` wrappers.
    pub fn wait_for_condition<F>(
        mut self,
        description: impl Into<String>,
        predicate: F,
        timeout: Duration,
        poll_interval: Duration,
    ) -> Self
    where
        F: Fn(&App) -> Result<bool> + Send + Sync + 'static,
    {
        let condition =
            WaitCondition::new(description, Arc::new(predicate), timeout, poll_interval);
        debug!(
            journey = %self.journey.name,
            timeout_ms = timeout.as_millis(),
            poll_interval_ms = poll_interval.as_millis(),
            "Registered wait-for-condition step"
        );
        if let Some(ref mut step) = self.current_step {
            step.follow_up_steps
                .push(TestStep::WaitForCondition(condition));
        } else {
            self.current_step = Some(JourneyStep {
                keys: Vec::new(),
                expected_scene: None,
                assertions: Vec::new(),
                follow_up_steps: vec![TestStep::WaitForCondition(condition)],
            });
        }
        self
    }

    /// Run an arbitrary assertion against the current application state.
    /// Pair with custom validation logic when built-in assertions are insufficient.
    pub fn assert_app_state<F>(mut self, description: impl Into<String>, predicate: F) -> Self
    where
        F: Fn(&App) -> Result<()> + Send + Sync + 'static,
    {
        let assertion = StateAssertion::new(description, Arc::new(predicate));
        if let Some(ref mut step) = self.current_step {
            step.follow_up_steps.push(TestStep::AssertState(assertion));
        } else {
            self.current_step = Some(JourneyStep {
                keys: Vec::new(),
                expected_scene: None,
                assertions: Vec::new(),
                follow_up_steps: vec![TestStep::AssertState(assertion)],
            });
        }
        self
    }

    /// Ensure the currently displayed error popup contains the provided substring.
    /// Combine with `.with_node_action_response` scripts that surface errors.
    pub fn expect_error_popup_contains(self, snippet: &str) -> Self {
        let snippet = snippet.to_string();
        self.assert_app_state(
            format!("Expect error popup to contain `{snippet}`"),
            move |app| {
                let status = status_component(app)?;
                let popup = status
                    .error_popup()
                    .filter(|popup| popup.is_visible())
                    .ok_or_else(|| eyre!("Error popup not visible"))?;
                if popup.message().contains(&snippet) || popup.error_message().contains(&snippet) {
                    Ok(())
                } else {
                    Err(eyre!(
                        "Error popup missing snippet `{}` (message='{}', error='{}')",
                        snippet,
                        popup.message(),
                        popup.error_message()
                    ))
                }
            },
        )
    }

    /// Assert a node's lifecycle state and whether it is locked by a transition.
    /// Often paired with `.assert_spinner` or `.expect_reachability` for richer checks.
    pub fn expect_node_state(self, node_id: &str, lifecycle: LifecycleState, locked: bool) -> Self {
        let node_id = node_id.to_string();
        self.assert_app_state(
            format!("Expect node `{node_id}` lifecycle {lifecycle:?} locked {locked}",),
            move |app| {
                let model = node_view_model(app, &node_id)?;
                if model.lifecycle != lifecycle {
                    return Err(eyre!(
                        "Node `{}` lifecycle mismatch: expected {:?}, found {:?}",
                        node_id,
                        lifecycle,
                        model.lifecycle
                    ));
                }
                if model.locked != locked {
                    return Err(eyre!(
                        "Node `{}` lock state mismatch: expected {}, found {}",
                        node_id,
                        locked,
                        model.locked
                    ));
                }
                Ok(())
            },
        )
    }

    /// Verify whether a node is currently displaying a spinner.
    /// Use after `.with_node_action_response` to confirm pending transitions.
    pub fn assert_spinner(self, node_id: &str, spinning: bool) -> Self {
        let node_id = node_id.to_string();
        self.assert_app_state(format!("Assert spinner for `{node_id}`"), move |app| {
            let model = node_view_model(app, &node_id)?;
            let is_spinning = model.pending_command.is_some();
            if is_spinning != spinning {
                return Err(eyre!(
                    "Node `{}` spinner state mismatch: expected {}, found {}",
                    node_id,
                    spinning,
                    is_spinning
                ));
            }
            Ok(())
        })
    }

    /// Assert reachability progress and status for a node view model.
    /// Works well with `.wait_for_reachability` when expecting async metrics updates.
    pub fn expect_reachability(
        self,
        node_id: &str,
        progress: ReachabilityProgress,
        status: ReachabilityStatusValues,
    ) -> Self {
        let node_id = node_id.to_string();
        self.assert_app_state(format!("Expect reachability for `{node_id}`"), move |app| {
            let model = node_view_model(app, &node_id)?;
            if model.reachability_progress != progress {
                return Err(eyre!(
                    "Node `{}` reachability progress mismatch: expected {:?}, found {:?}",
                    node_id,
                    progress,
                    model.reachability_progress
                ));
            }
            if model.reachability_status != status {
                return Err(eyre!("Node `{}` reachability status mismatch", node_id));
            }
            Ok(())
        })
    }

    /// Wait for a node to reach the specified lifecycle state.
    /// Combine with `.with_node_action_response` to synchronise on mock transitions.
    pub fn wait_for_node_state(
        self,
        node_id: &str,
        lifecycle: LifecycleState,
        timeout: Duration,
        poll_interval: Duration,
    ) -> Self {
        let node_id = node_id.to_string();
        self.wait_for_condition(
            format!("Wait for node `{node_id}` lifecycle {lifecycle:?}"),
            move |app| {
                let model = node_view_model(app, &node_id)?;
                Ok(model.lifecycle == lifecycle)
            },
            timeout,
            poll_interval,
        )
    }

    /// Wait for a node's reachability progress to reach the target state.
    /// Pair with `.with_metrics_script` or `.then_metrics` in response plans.
    pub fn wait_for_reachability(
        self,
        node_id: &str,
        progress: ReachabilityProgress,
        timeout: Duration,
        poll_interval: Duration,
    ) -> Self {
        let node_id = node_id.to_string();
        self.wait_for_condition(
            format!("Wait for reachability `{node_id}` to {progress:?}"),
            move |app| {
                let model = node_view_model(app, &node_id)?;
                Ok(model.reachability_progress == progress)
            },
            timeout,
            poll_interval,
        )
    }

    /// Finalise the currently staged step and enqueue it for execution.
    ///
    /// Each fluent call (`press`, `expect_text`, `wait`, …) accumulates work in
    /// `current_step`. Calling `step()` commits that buffered input so the
    /// runtime processes it before you start describing the next phase of the
    /// journey. Skipping `step()` does **not** lose the actions—the builder calls
    /// it automatically inside `build`/`run`—but the tests become harder to read
    /// because all the interactions collapse into a single opaque block. Treat it
    /// as “end of paragraph” punctuation when scripting multi-stage scenarios.
    pub fn step(mut self) -> Self {
        if let Some(step) = self.current_step.take() {
            debug!(
                journey = %self.journey.name,
                key_events = step.keys.len(),
                assertions = step.assertions.len(),
                follow_ups = step.follow_up_steps.len(),
                has_scene_expectation = step.expected_scene.is_some(),
                "Queued journey step"
            );
            self.journey.add_step(step);
        }
        self
    }

    /// Build and execute the journey, returning any error from runtime execution.
    /// Use when the scripted steps should run immediately.
    pub async fn run(self) -> Result<()> {
        let mut journey = self.build()?;
        journey.run().await
    }

    /// Convert the builder into an executable `Journey`.
    /// Enables manual reuse of the constructed journey before calling `.run`.
    pub fn build(mut self) -> Result<Journey> {
        if let Some(step) = self.current_step.take() {
            self.journey.add_step(step);
        }

        let step_count = self.journey.steps.len();
        let action_count = self.journey.build_test_script().len();
        let scripted_commands = self.node_action_scripts.len();
        info!(
            journey = %self.journey.name,
            step_count,
            action_count,
            scripted_commands,
            "Finalising journey build"
        );

        if let Some(handle) = self.journey.take_node_management_handle() {
            if self.node_action_scripts.is_empty() {
                debug!(journey = %self.journey.name, "Reusing existing node-management handle");
                self.journey.node_management_handle = Some(handle);
            } else {
                let actions = std::mem::take(&mut self.node_action_scripts);
                debug!(
                    journey = %self.journey.name,
                    scripts = actions.len(),
                    "Spawning scripted node-management task"
                );
                let scripted_task = handle.spawn_script(actions);
                self.journey.register_script_task(scripted_task);
            }
        } else if !self.node_action_scripts.is_empty() {
            // No handle available to run scripts; log for visibility.
            tracing::warn!(
                "Node action scripts configured but no mock node-management handle provided"
            );
        }

        Ok(self.journey)
    }
}

impl From<char> for KeySequence {
    fn from(c: char) -> Self {
        KeySequence::new().key(c)
    }
}
