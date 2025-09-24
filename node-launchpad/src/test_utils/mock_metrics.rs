// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::action::Action;
use crate::components::node_table::lifecycle::RegistryNode;
use crate::node_stats::{AggregatedNodeStats, MetricsFetcher};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc::UnboundedSender;
use tracing::error;

pub struct MockMetricsService;

struct ScriptedMetricsFetcher {
    script: Mutex<VecDeque<AggregatedNodeStats>>,
}

impl ScriptedMetricsFetcher {
    fn new(script: Vec<AggregatedNodeStats>) -> Self {
        Self {
            script: Mutex::new(script.into()),
        }
    }
}

impl MetricsFetcher for ScriptedMetricsFetcher {
    fn fetch(&self, _nodes: Vec<RegistryNode>, sender: UnboundedSender<Action>) {
        match self.script.lock() {
            Ok(mut script) => {
                if let Some(stats) = script.pop_front() {
                    let _ = sender.send(Action::StoreAggregatedNodeStats(stats));
                }
            }
            Err(err) => {
                error!("Failed to acquire scripted metrics lock: {err}");
            }
        }
    }
}

impl MockMetricsService {
    pub fn scripted(script: Vec<AggregatedNodeStats>) -> Arc<dyn MetricsFetcher> {
        Arc::new(ScriptedMetricsFetcher::new(script))
    }
}
