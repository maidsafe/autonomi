// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::action::{Action, NodeManagementCommand, NodeManagementResponse, NodeTableActions};
use crate::node_management::{NodeManagementHandle, NodeManagementTask};
use crate::node_stats::AggregatedNodeStats;
use ant_service_management::{NodeRegistryManager, NodeServiceData};
use color_eyre::eyre::{Result, eyre};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio::sync::{RwLock, mpsc};
use tokio::task::JoinHandle;
use tokio::time::{Duration, sleep};
use tracing::{debug, error, info, warn};

/// Script describing how the mock node-management service should respond.
///
/// The plan works as a miniature DSL for modelling the asynchronous behaviour of
/// the real node-management pipeline. A plan can optionally wait, emit a single
/// [`NodeManagementResponse`], and replay a sequence of follow-up events—each of
/// which maps to something the production launchpad would observe (registry
/// updates via the watcher, metrics snapshots, or arbitrary `Action`s). By
/// chaining the builder helpers (`then_registry_snapshot`, `then_metrics`,
/// `then_actions`, `then_wait`) journey tests remain declarative while still
/// exercising the exact UI update paths triggered by antctl.
///
/// Typical usage:
/// ```ignore
/// let plan = MockNodeResponsePlan::immediate(NodeManagementResponse::MaintainNodes { error: None })
///     .then_registry_snapshot(vec![node_added])
///     .then_metrics([progress_metrics])
///     .then_wait(Duration::from_millis(200))
///     .then_registry_snapshot(vec![node_running])
///     .then_metrics([completion_metrics]);
///
/// JourneyBuilder::from_context("scenario", test_app)?
///     .with_node_action_response(NodeManagementCommand::MaintainNodes, plan)
///     .build()?;
/// ```
///
/// The scripted executor inside [`MockNodeManagementHandle::spawn_script`] will read
/// the plan and feed its events back into the application under test.
#[derive(Clone, Debug, Default)]
pub struct MockNodeResponsePlan {
    pub delay: Duration,
    pub response: Option<NodeManagementResponse>,
    pub followup_events: Vec<PlannedEvent>,
}

/// Follow-up events that the scripted executor will replay after the initial response.
///
/// Events are processed sequentially; waits pause execution, actions are sent to the
/// application directly, and registry snapshots persist data so the watcher refreshes the UI.
#[derive(Clone, Debug)]
pub enum PlannedEvent {
    Wait(Duration),
    Action(Action),
    RegistrySnapshot(Vec<NodeServiceData>),
}

impl MockNodeResponsePlan {
    /// Immediately return the supplied response without scheduling follow-up actions.
    /// Ideal for simple success/error cases when no extra actions are needed.
    pub fn immediate(response: NodeManagementResponse) -> Self {
        Self {
            response: Some(response),
            ..Default::default()
        }
    }

    /// Legacy helper for building a plan with explicit follow-up actions.
    /// Prefer chaining `then_metrics`/`then_registry_snapshot` for clarity.
    pub fn with_follow_up(response: NodeManagementResponse, followup_actions: Vec<Action>) -> Self {
        Self {
            response: Some(response),
            followup_events: followup_actions
                .into_iter()
                .map(PlannedEvent::Action)
                .collect(),
            ..Default::default()
        }
    }

    /// Introduce an artificial delay before emitting the response and actions.
    /// Combine with `then_metrics` to simulate asynchronous updates.
    pub fn with_delay(mut self, delay: Duration) -> Self {
        self.delay = delay;
        self
    }

    /// Append additional side-effect actions.
    /// Use for custom `Action` values not covered by the dedicated helpers.
    pub fn then_actions<I>(mut self, actions: I) -> Self
    where
        I: IntoIterator<Item = Action>,
    {
        self.followup_events
            .extend(actions.into_iter().map(PlannedEvent::Action));
        self
    }

    /// Convenience for persisting a registry snapshot that will be picked up by the watcher.
    /// Pair with node builders such as `make_node_service_data` to mirror registry snapshots.
    pub fn then_registry_snapshot(mut self, nodes: Vec<NodeServiceData>) -> Self {
        self.followup_events
            .push(PlannedEvent::RegistrySnapshot(nodes));
        self
    }

    /// Queue metrics updates that will be fed into the app's stats reducer.
    /// Works well alongside `with_metrics_events` so tests observe consistent data.
    pub fn then_metrics<I>(mut self, stats: I) -> Self
    where
        I: IntoIterator<Item = AggregatedNodeStats>,
    {
        self.followup_events.extend(
            stats
                .into_iter()
                .map(|stat| PlannedEvent::Action(Action::StoreAggregatedNodeStats(stat))),
        );
        self
    }

    /// Schedule a delay before the next follow-up event is dispatched.
    /// Useful when modelling asynchronous registry watcher updates in tests.
    pub fn then_wait(mut self, duration: Duration) -> Self {
        self.followup_events.push(PlannedEvent::Wait(duration));
        self
    }
}

/// Pairing of a node-management command with the scripted plan that should answer it.
/// Produced automatically by `JourneyBuilder::with_node_action_response`.
#[derive(Clone, Debug)]
pub struct ScriptedNodeAction {
    pub command: NodeManagementCommand,
    pub plan: MockNodeResponsePlan,
}

#[derive(Clone)]
pub struct MockNodeManagement {
    task_tx: UnboundedSender<NodeManagementTask>,
    state: Arc<Mutex<MockState>>,
}

struct MockState {
    action_sender: Option<UnboundedSender<Action>>,
    node_registry: Option<NodeRegistryManager>,
}

pub struct MockNodeManagementHandle {
    task_rx: UnboundedReceiver<NodeManagementTask>,
    state: Arc<Mutex<MockState>>,
}

impl MockNodeManagement {
    /// Create a mock node-management pair (service + handle).
    /// Feed the handle into `TestAppBuilder` or `JourneyBuilder` when you need full manual control.
    pub fn new() -> (Arc<Self>, MockNodeManagementHandle) {
        Self::new_with_registry(None)
    }

    pub fn new_with_registry(
        node_registry: Option<NodeRegistryManager>,
    ) -> (Arc<Self>, MockNodeManagementHandle) {
        let (task_tx, task_rx) = mpsc::unbounded_channel();
        let state = Arc::new(Mutex::new(MockState {
            action_sender: None,
            node_registry,
        }));
        let management = Arc::new(Self {
            task_tx,
            state: Arc::clone(&state),
        });

        let handle = MockNodeManagementHandle { task_rx, state };

        (management, handle)
    }
}

impl NodeManagementHandle for MockNodeManagement {
    fn send_task(&self, task: NodeManagementTask) -> Result<()> {
        if let NodeManagementTask::RegisterActionSender { action_sender } = &task {
            if let Ok(mut state) = self.state.lock() {
                state.action_sender = Some(action_sender.clone());
            } else {
                return Err(eyre!("failed to lock mock node-management state"));
            }
        }
        self.task_tx
            .send(task)
            .map_err(|err| eyre!("failed to dispatch mock node-management task: {err}"))
    }
}

impl MockNodeManagementHandle {
    /// Await the next task dispatched by the application under test.
    /// Combine with `spawn_script` when building bespoke async orchestration.
    pub async fn recv_task(&mut self) -> Option<NodeManagementTask> {
        self.task_rx.recv().await
    }

    fn registry_manager(&self) -> Option<NodeRegistryManager> {
        self.state.lock().ok()?.node_registry.clone()
    }

    /// Send a `NodeManagementResponse` back into the application.
    /// Use alongside `MockNodeResponsePlan::immediate` when steering flows manually.
    pub fn respond(&self, response: NodeManagementResponse) -> Result<()> {
        let sender = self
            .state
            .lock()
            .map_err(|err| eyre!("failed to lock mock node-management state: {err}"))?
            .action_sender
            .clone()
            .ok_or_else(|| eyre!("no action sender registered"))?;

        sender
            .send(Action::NodeTableActions(
                NodeTableActions::NodeManagementResponse(response),
            ))
            .map_err(|err| eyre!("failed to send mock node-management response: {err}"))
    }

    /// Inject an arbitrary action onto the app's event stream.
    /// Pair with `MockNodeResponsePlan::then_actions` when composing advanced scripts.
    pub fn send_action(&self, action: Action) -> Result<()> {
        let sender = self
            .state
            .lock()
            .map_err(|err| eyre!("failed to lock mock node-management state: {err}"))?
            .action_sender
            .clone()
            .ok_or_else(|| eyre!("no action sender registered"))?;

        sender
            .send(action)
            .map_err(|err| eyre!("failed to send action through mock node-management: {err}"))
    }

    /// Spawn a background task that replays scripted responses sequentially.
    /// Typically driven via `JourneyBuilder::with_node_action_response`.
    pub fn spawn_script(self, script: Vec<ScriptedNodeAction>) -> JoinHandle<()> {
        tokio::spawn(async move {
            let mut script_queue = script;
            let mut handle = self;
            while let Some(task) = handle.recv_task().await {
                if let Some(command) = command_from_task(&task) {
                    if let Some((index, _)) = script_queue
                        .iter()
                        .enumerate()
                        .find(|(_, entry)| entry.command == command)
                    {
                        let scripted = script_queue.remove(index);
                        let plan = scripted.plan;
                        let delay_ms = plan.delay.as_millis();
                        let follow_ups = plan.followup_events.len();
                        let has_response = plan.response.is_some();
                        info!(
                            ?command,
                            delay_ms,
                            follow_ups,
                            has_response,
                            "Executing scripted node-management plan"
                        );
                        if plan.delay.as_millis() > 0 {
                            sleep(plan.delay).await;
                        }

                        let events = plan.followup_events.into_iter();

                        if let Some(response) = plan.response
                            && let Err(err) = handle.respond(response)
                        {
                            error!("Failed to send scripted node-management response: {err}");
                        }

                        for event in events {
                            match event {
                                PlannedEvent::Wait(duration) => sleep(duration).await,
                                PlannedEvent::Action(action) => {
                                    if let Err(err) = handle.send_action(action) {
                                        error!("Failed to send scripted follow-up action: {err}");
                                    }
                                }
                                PlannedEvent::RegistrySnapshot(nodes) => {
                                    if let Some(registry) = handle.registry_manager() {
                                        if let Err(err) =
                                            persist_registry_snapshot(registry, nodes.clone()).await
                                        {
                                            warn!("Failed to persist registry snapshot: {err}");
                                        }
                                    } else {
                                        warn!(
                                            "Registry snapshot provided but no registry manager is attached"
                                        );
                                    }
                                }
                            }
                        }
                    } else {
                        match command {
                            NodeManagementCommand::RefreshRegistry => {
                                debug!("Auto-responding to RefreshRegistry command");
                                if let Err(err) =
                                    handle.respond(NodeManagementResponse::RefreshRegistry {
                                        error: None,
                                    })
                                {
                                    error!("Failed to send auto RefreshRegistry response: {err}");
                                }
                            }
                            _ => {
                                debug!("No scripted response registered for command: {command:?}");
                            }
                        }
                    }
                } else {
                    debug!("Ignoring non-command node-management task: {task:?}");
                }
            }

            if !script_queue.is_empty() {
                warn!(
                    "Scripted node-management actions not executed: {:?}",
                    script_queue
                        .into_iter()
                        .map(|entry| entry.command)
                        .collect::<Vec<_>>()
                );
            }
        })
    }
}

/// Translate a raw node-management task into its high-level command.
/// Internal helper used by the scripted mock executor spawned by `spawn_script`.
fn command_from_task(task: &NodeManagementTask) -> Option<NodeManagementCommand> {
    match task {
        NodeManagementTask::RegisterActionSender { .. } => None,
        NodeManagementTask::RefreshNodeRegistry { .. } => {
            Some(NodeManagementCommand::RefreshRegistry)
        }
        NodeManagementTask::MaintainNodes { .. } => Some(NodeManagementCommand::MaintainNodes),
        NodeManagementTask::ResetNodes => Some(NodeManagementCommand::ResetNodes),
        NodeManagementTask::StopNodes { .. } => Some(NodeManagementCommand::StopNodes),
        NodeManagementTask::UpgradeNodes { .. } => Some(NodeManagementCommand::UpgradeNodes),
        NodeManagementTask::AddNode { .. } => Some(NodeManagementCommand::AddNode),
        NodeManagementTask::RemoveNodes { .. } => Some(NodeManagementCommand::RemoveNodes),
        NodeManagementTask::StartNode { .. } => Some(NodeManagementCommand::StartNodes),
    }
}

async fn persist_registry_snapshot(
    registry: NodeRegistryManager,
    nodes: Vec<NodeServiceData>,
) -> Result<()> {
    {
        let mut registry_nodes = registry.nodes.write().await;
        registry_nodes.clear();
        registry_nodes.extend(nodes.into_iter().map(|node| Arc::new(RwLock::new(node))));
    }
    registry.save().await?;
    Ok(())
}
