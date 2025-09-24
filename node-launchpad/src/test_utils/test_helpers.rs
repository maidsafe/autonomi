// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use super::{
    mock_metrics::MockMetricsService,
    mock_node_management::{MockNodeManagement, MockNodeManagementHandle},
};
use crate::node_management::NodeManagementHandle;
use crate::test_utils::make_node_service_data;
use crate::{
    app::App,
    config::AppData,
    node_stats::{AggregatedNodeStats, MetricsFetcher},
};
use ant_bootstrap::InitialPeersConfig;
use ant_service_management::{NodeRegistryManager, NodeServiceData, ServiceStatus};
use color_eyre::eyre::Result;
use ratatui::{Terminal, backend::TestBackend, buffer::Buffer};
use std::{iter, sync::Arc};
use tempfile::TempDir;

pub struct TestAppBuilder {
    node_management: Option<Arc<dyn NodeManagementHandle>>,
    node_registry: Option<NodeRegistryManager>,
    initial_nodes: Vec<NodeServiceData>,
    nodes_to_start: Option<u64>,
    metrics_fetcher: Option<Arc<dyn MetricsFetcher>>,
}

impl TestAppBuilder {
    pub fn new() -> Self {
        Self {
            node_management: None,
            node_registry: None,
            initial_nodes: Vec::new(),
            nodes_to_start: None,
            metrics_fetcher: None,
        }
    }

    pub fn with_initial_node(mut self, node: NodeServiceData) -> Self {
        self.initial_nodes.push(node);
        self
    }

    pub fn with_initial_nodes<I>(mut self, nodes: I) -> Self
    where
        I: IntoIterator<Item = NodeServiceData>,
    {
        self.initial_nodes.extend(nodes);
        self
    }

    pub fn with_nodes<I>(mut self, statuses: I) -> Self
    where
        I: IntoIterator<Item = ServiceStatus>,
    {
        let offset = self.initial_nodes.len() as u64;
        for (idx, status) in statuses.into_iter().enumerate() {
            let node = make_node_service_data(offset + idx as u64, status);
            self.initial_nodes.push(node);
        }
        self
    }

    pub fn with_running_nodes(self, count: u64) -> Self {
        self.with_nodes(iter::repeat_n(ServiceStatus::Running, count as usize))
    }

    pub fn with_stopped_nodes(self, count: u64) -> Self {
        self.with_nodes(iter::repeat_n(ServiceStatus::Stopped, count as usize))
    }

    pub fn with_node_management(mut self, node_management: Arc<dyn NodeManagementHandle>) -> Self {
        self.node_management = Some(node_management);
        self
    }

    pub fn with_node_registry(mut self, registry: NodeRegistryManager) -> Self {
        self.node_registry = Some(registry);
        self
    }

    pub fn with_nodes_to_start(mut self, nodes: u64) -> Self {
        self.nodes_to_start = Some(nodes);
        self
    }

    pub fn with_metrics_events<I>(mut self, stats: I) -> Self
    where
        I: IntoIterator<Item = AggregatedNodeStats>,
    {
        let script: Vec<_> = stats.into_iter().collect();
        self.metrics_fetcher = Some(MockMetricsService::scripted(script));
        self
    }

    pub fn with_metrics_script(self, script: Vec<AggregatedNodeStats>) -> Self {
        self.with_metrics_events(script)
    }

    pub async fn build(self) -> Result<TestAppContext> {
        let Self {
            node_management,
            node_registry,
            mut initial_nodes,
            nodes_to_start,
            metrics_fetcher,
        } = self;

        let mut node_management = node_management;
        let mut mock_node_management: Option<Arc<MockNodeManagement>> = None;
        let mut mock_node_management_handle: Option<MockNodeManagementHandle> = None;

        if node_management.is_none() {
            let (mock, handle) = MockNodeManagement::new();
            mock_node_management_handle = Some(handle);
            mock_node_management = Some(Arc::clone(&mock));
            let dyn_handle: Arc<dyn NodeManagementHandle> = mock;
            node_management = Some(dyn_handle);
        }

        let (node_registry_manager, registry_dir): (NodeRegistryManager, Option<TempDir>) =
            match node_registry {
                Some(manager) => (manager, None),
                None => {
                    let dir = TempDir::new()?;
                    let path = dir.path().join("node_registry.json");
                    (NodeRegistryManager::empty(path), Some(dir))
                }
            };

        for node in initial_nodes.drain(..) {
            node_registry_manager.push_node(node).await;
        }

        if registry_dir.is_some() {
            node_registry_manager.save().await?;
        }

        let seeded_node_count = node_registry_manager.get_node_service_data().await.len() as u64;
        let nodes_to_start = nodes_to_start.unwrap_or(seeded_node_count);

        let app_data = AppData {
            rewards_address: Some(crate::test_utils::TEST_WALLET_ADDRESS.parse()?),
            nodes_to_start,
            storage_mountpoint: None,
            storage_drive: Some(crate::test_utils::TEST_STORAGE_DRIVE.to_string()),
            upnp_enabled: true,
            port_range: None,
        };

        let app = App::new_with_dependencies(
            1.0,  // tick_rate
            60.0, // frame_rate
            InitialPeersConfig::default(),
            None, // antnode_path
            None,
            Some(1), // network_id
            node_management,
            Some(node_registry_manager.clone()),
            Some(app_data),
            false,
            metrics_fetcher,
        )
        .await?;

        Ok(TestAppContext {
            app,
            node_management_handle: mock_node_management_handle,
            mock_node_management,
            registry_dir,
        })
    }
}

pub struct TestAppContext {
    pub app: App,
    pub node_management_handle: Option<MockNodeManagementHandle>,
    pub mock_node_management: Option<Arc<MockNodeManagement>>,
    pub registry_dir: Option<TempDir>,
}

impl Default for TestAppBuilder {
    fn default() -> Self {
        Self::new()
    }
}

pub fn render_status_component(app: &mut App) -> Result<Buffer> {
    let mut terminal = Terminal::new(TestBackend::new(160, 30))?;

    terminal.draw(|f| {
        if let Some(status) = app.components.get_mut(0)
            && let Err(e) = status.draw(f, f.area())
        {
            eprintln!("Failed to render Status component: {e}");
        }
    })?;

    Ok(terminal.backend().buffer().clone())
}

pub fn buffer_to_lines(buffer: &Buffer) -> Vec<String> {
    let width = buffer.area.width as usize;
    let height = buffer.area.height as usize;
    let mut lines = Vec::new();

    for y in 0..height {
        let mut line = String::new();
        for x in 0..width {
            let index = y * width + x;
            if index < buffer.content().len() {
                let cell = &buffer.content()[index];
                line.push_str(cell.symbol());
            }
        }
        lines.push(line);
    }

    lines
}
