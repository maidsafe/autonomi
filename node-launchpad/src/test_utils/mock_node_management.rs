// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::action::{Action, NodeManagementCommand, NodeManagementResponse, NodeTableActions};
use crate::node_management::{NodeManagementHandle, NodeManagementTask};
use color_eyre::eyre::{Result, eyre};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio::task::JoinHandle;
use tokio::time::{Duration, sleep};
use tracing::{debug, error, warn};

#[derive(Clone, Debug, Default)]
pub struct MockResponsePlan {
    pub delay: Duration,
    pub response: Option<NodeManagementResponse>,
    pub followup_actions: Vec<Action>,
}

impl MockResponsePlan {
    pub fn immediate(response: NodeManagementResponse) -> Self {
        Self {
            response: Some(response),
            ..Default::default()
        }
    }

    pub fn with_follow_up(response: NodeManagementResponse, followup_actions: Vec<Action>) -> Self {
        Self {
            response: Some(response),
            followup_actions,
            ..Default::default()
        }
    }

    pub fn with_delay(mut self, delay: Duration) -> Self {
        self.delay = delay;
        self
    }
}

#[derive(Clone, Debug)]
pub struct ScriptedNodeAction {
    pub command: NodeManagementCommand,
    pub plan: MockResponsePlan,
}

#[derive(Clone)]
pub struct MockNodeManagement {
    task_tx: UnboundedSender<NodeManagementTask>,
    state: Arc<Mutex<MockState>>,
}

struct MockState {
    action_sender: Option<UnboundedSender<Action>>,
}

pub struct MockNodeManagementHandle {
    task_rx: UnboundedReceiver<NodeManagementTask>,
    state: Arc<Mutex<MockState>>,
}

impl MockNodeManagement {
    pub fn new() -> (Arc<Self>, MockNodeManagementHandle) {
        let (task_tx, task_rx) = mpsc::unbounded_channel();
        let state = Arc::new(Mutex::new(MockState {
            action_sender: None,
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
    pub async fn recv_task(&mut self) -> Option<NodeManagementTask> {
        self.task_rx.recv().await
    }

    pub fn try_recv_task(&mut self) -> Option<NodeManagementTask> {
        self.task_rx.try_recv().ok()
    }

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
                        if plan.delay.as_millis() > 0 {
                            sleep(plan.delay).await;
                        }

                        if let Some(response) = plan.response
                            && let Err(err) = handle.respond(response)
                        {
                            error!("Failed to send scripted node-management response: {err}");
                        }

                        for action in plan.followup_actions {
                            if let Err(err) = handle.send_action(action) {
                                error!("Failed to send scripted follow-up action: {err}");
                            }
                        }
                    } else {
                        match command {
                            NodeManagementCommand::RefreshRegistry => {
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
