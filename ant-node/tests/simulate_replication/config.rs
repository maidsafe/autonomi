// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! Configuration constants for the replication simulation.

use ant_protocol::CLOSE_GROUP_SIZE;

// ============================================================================
// Constants (from codebase)
// ============================================================================

pub const LIBP2P_K_VALUE: usize = 20; // Max peers per Kademlia bucket (from rust-libp2p)
pub const MAX_RECORDS_COUNT: usize = 16384;
pub const REPLICATION_MAJORITY_THRESHOLD: usize = CLOSE_GROUP_SIZE / 2; // 2 peers required

// ============================================================================
// Simulation Configuration
// ============================================================================

pub const SIMULATION_NUM_NODES: usize = 1000;
pub const SIMULATION_NUM_CHUNKS: usize = 1000;
pub const SIMULATION_REPLICATION_ROUNDS: usize = 10;
pub const SIMULATION_FAILURE_RATE: f64 = 0.10; // 10% of nodes fail after replication

// ============================================================================
// Monte Carlo Configuration
// ============================================================================

pub const MONTE_CARLO_TRIALS: usize = 10; // Number of simulation trials for statistical confidence
pub const MONTE_CARLO_ENABLED: bool = false; // Set to true to run Monte Carlo mode

#[derive(Debug)]
#[allow(dead_code)]
pub enum PaymentMode {
    SingleNode, // Pay 1 node (index 2) with 3x amount
    Standard,   // Pay 3 nodes (indices 2, 3, 4) with quoted amounts
}

pub const SIMULATION_PAYMENT_MODE: PaymentMode = PaymentMode::Standard;
