// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! JSON export structures for simulation results.

use serde::Serialize;

// ============================================================================
// JSON Export Structures
// ============================================================================

#[derive(Serialize)]
pub struct SimulationExport {
    pub config: SimulationConfig,
    pub rounds: Vec<RoundData>,
    pub final_coverage: CoverageExport,
    pub fault_tolerance: FaultToleranceMetrics,
    pub chunk_timelines: Vec<ChunkTimeline>,
}

#[derive(Serialize)]
pub struct SimulationConfig {
    pub num_nodes: usize,
    pub num_chunks: usize,
    pub replication_rounds: usize,
    pub close_group_size: usize,
    pub k_value: usize,
    pub majority_threshold: usize,
}

#[derive(Serialize)]
pub struct RoundData {
    pub round_number: usize,
    pub total_records: usize,
    pub replications: usize,
    pub avg_records_per_node: f64,
}

#[derive(Serialize)]
pub struct CoverageExport {
    pub total_chunks: usize,
    pub distribution: [usize; 8], // 0/7 through 7/7
    pub average_percent: f64,
}

#[derive(Serialize)]
pub struct ChunkTimeline {
    pub address: String,
    pub holders_per_round: Vec<usize>,
    pub final_coverage: usize,
}

#[derive(Serialize, Clone)]
pub struct FaultToleranceMetrics {
    pub data_loss_rate: f64,
    pub critical_risk_rate: f64,
    pub mean_coverage: f64,
    pub coverage_std_dev: f64,
    pub p99_coverage: usize,
    pub survival_probability: f64,
}
