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

#[derive(Serialize, Clone, Default)]
pub struct FaultToleranceMetrics {
    pub data_loss_rate: f64,
    pub critical_risk_rate: f64,
    pub mean_coverage: f64,
    pub coverage_std_dev: f64,
    pub p99_coverage: usize,
    pub survival_probability: f64,
}

// ============================================================================
// Monte Carlo Results
// ============================================================================

#[derive(Serialize)]
pub struct MonteCarloExport {
    pub num_trials: usize,
    pub config: SimulationConfig,
    pub summary: MonteCarloSummary,
    pub trials: Vec<TrialResult>,
}

#[derive(Serialize)]
pub struct MonteCarloSummary {
    pub data_loss_rate: ConfidenceInterval,
    pub critical_risk_rate: ConfidenceInterval,
    pub mean_coverage: ConfidenceInterval,
    pub survival_probability: ConfidenceInterval,
}

#[derive(Serialize)]
pub struct ConfidenceInterval {
    pub mean: f64,
    pub std_dev: f64,
    pub ci_95_lower: f64,
    pub ci_95_upper: f64,
    pub min: f64,
    pub max: f64,
}

impl ConfidenceInterval {
    pub fn from_samples(samples: &[f64]) -> Self {
        let n = samples.len() as f64;
        if n == 0.0 {
            return Self {
                mean: 0.0,
                std_dev: 0.0,
                ci_95_lower: 0.0,
                ci_95_upper: 0.0,
                min: 0.0,
                max: 0.0,
            };
        }

        let mean = samples.iter().sum::<f64>() / n;
        let variance = samples.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n;
        let std_dev = variance.sqrt();

        // 95% CI: mean ± 1.96 * (std_dev / sqrt(n))
        let std_error = std_dev / n.sqrt();
        let ci_margin = 1.96 * std_error;

        let min = samples.iter().cloned().fold(f64::INFINITY, f64::min);
        let max = samples.iter().cloned().fold(f64::NEG_INFINITY, f64::max);

        Self {
            mean,
            std_dev,
            ci_95_lower: mean - ci_margin,
            ci_95_upper: mean + ci_margin,
            min,
            max,
        }
    }
}

#[derive(Serialize, Clone)]
pub struct TrialResult {
    pub trial_number: usize,
    pub fault_tolerance: FaultToleranceMetrics,
    pub coverage_percent: f64,
}
