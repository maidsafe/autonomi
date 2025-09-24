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
    mock_registry::MockNodeRegistry,
};
use crate::node_management::NodeManagementHandle;
use crate::{
    app::App,
    config::AppData,
    node_stats::{AggregatedNodeStats, MetricsFetcher},
};
use ant_bootstrap::InitialPeersConfig;
use ant_service_management::NodeRegistryManager;
use color_eyre::eyre::Result;
use ratatui::{Terminal, backend::TestBackend, buffer::Buffer};
use std::sync::Arc;

pub struct TestAppBuilder {
    mock_registry: Option<MockNodeRegistry>,
    node_management: Option<Arc<dyn NodeManagementHandle>>,
    node_registry: Option<NodeRegistryManager>,
    nodes_to_start: Option<u64>,
    metrics_fetcher: Option<Arc<dyn MetricsFetcher>>,
    mock_node_management_handle: Option<MockNodeManagementHandle>,
    mock_node_management: Option<Arc<MockNodeManagement>>,
}

impl TestAppBuilder {
    pub fn new() -> Self {
        Self {
            mock_registry: None,
            node_management: None,
            node_registry: None,
            nodes_to_start: None,
            metrics_fetcher: None,
            mock_node_management_handle: None,
            mock_node_management: None,
        }
    }

    pub fn with_mock_registry(mut self, registry: MockNodeRegistry) -> Self {
        self.mock_registry = Some(registry);
        self
    }

    pub fn with_node_management(mut self, node_management: Arc<dyn NodeManagementHandle>) -> Self {
        self.node_management = Some(node_management);
        self
    }

    pub fn with_mock_node_management(
        mut self,
        node_management: Arc<MockNodeManagement>,
        handle: MockNodeManagementHandle,
    ) -> Self {
        let dyn_handle = Arc::clone(&node_management);
        self.mock_node_management = Some(node_management);
        self.node_management = Some(dyn_handle);
        self.mock_node_management_handle = Some(handle);
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

    pub fn with_metrics_script(mut self, script: Vec<AggregatedNodeStats>) -> Self {
        self.metrics_fetcher = Some(MockMetricsService::scripted(script));
        self
    }

    pub async fn build(self) -> Result<TestAppContext> {
        let Self {
            mock_registry,
            node_management,
            node_registry,
            nodes_to_start,
            metrics_fetcher,
            mock_node_management_handle,
            mock_node_management,
        } = self;

        let registry = match mock_registry {
            Some(registry) => registry,
            None => MockNodeRegistry::empty()?,
        };

        let registry_path = registry.get_registry_path().clone();

        let node_registry_manager = match node_registry {
            Some(manager) => manager,
            None => NodeRegistryManager::load(&registry_path).await?,
        };

        let nodes_to_start = nodes_to_start.unwrap_or_else(|| registry.node_count());

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
            registry,
            node_management_handle: mock_node_management_handle,
            mock_node_management,
        })
    }
}

pub struct TestAppContext {
    pub app: App,
    pub registry: MockNodeRegistry,
    pub node_management_handle: Option<MockNodeManagementHandle>,
    pub mock_node_management: Option<Arc<MockNodeManagement>>,
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
