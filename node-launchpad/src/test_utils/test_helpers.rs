// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::{app::App, config::AppData, connection_mode::ConnectionMode};
use ant_bootstrap::InitialPeersConfig;
use color_eyre::eyre::Result;
use ratatui::{Terminal, backend::TestBackend, buffer::Buffer};
use std::{
    env,
    sync::atomic::{AtomicU64, Ordering},
};

static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

pub struct TestAppBuilder {
    nodes_to_start: usize,
}

impl TestAppBuilder {
    pub fn new() -> Self {
        Self { nodes_to_start: 0 }
    }

    pub fn with_nodes(mut self, count: usize) -> Self {
        self.nodes_to_start = count;
        self
    }

    pub async fn build(self) -> Result<App> {
        let unique_id = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
        let temp_dir = env::temp_dir().join(format!(
            "launchpad_test_{}_{}",
            std::process::id(),
            unique_id
        ));

        let config_dir = temp_dir.join("config");
        std::fs::create_dir_all(&config_dir)?;

        let app_data = AppData {
            discord_username: crate::test_utils::TEST_WALLET_ADDRESS.to_string(),
            nodes_to_start: self.nodes_to_start,
            storage_mountpoint: None,
            storage_drive: Some(crate::test_utils::TEST_STORAGE_DRIVE.to_string()),
            connection_mode: Some(ConnectionMode::Automatic),
            port_from: None,
            port_to: None,
        };

        let config_path = config_dir.join("app_data.json");
        app_data.save(Some(config_path.clone()))?;

        App::new(
            1.0,  // tick_rate
            60.0, // frame_rate
            InitialPeersConfig::default(),
            None, // antnode_path
            Some(config_path),
            Some(1), // network_id
        )
        .await
    }
}

impl Default for TestAppBuilder {
    fn default() -> Self {
        Self::new()
    }
}

pub async fn render_status_component(app: &mut App) -> Result<Buffer> {
    let mut terminal = Terminal::new(TestBackend::new(160, 30)).unwrap();

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
