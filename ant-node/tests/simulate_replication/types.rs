// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! Core data structures for the replication simulation.

use ant_protocol::{NetworkAddress, storage::{DataTypes, RecordKind}};
use libp2p::{PeerId, kad::KBucketDistance as Distance};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::Duration;

use crate::config::REPLICATION_MAJORITY_THRESHOLD;

// ============================================================================
// Core Data Structures
// ============================================================================

/// A simulated node in the network
#[derive(Debug)]
pub struct SimulatedNode {
    pub peer_id: PeerId,
    pub address: NetworkAddress,

    // Local routing table: ilog2_distance -> list of peers
    pub routing_table: BTreeMap<u32, Vec<PeerId>>,

    // Stored records
    pub stored_records: HashMap<NetworkAddress, StoredRecord>,

    // Network state
    pub responsible_distance: Option<Distance>, // Distance range this node is responsible for
    pub network_size_estimate: usize,

    // Quote generation factors
    pub received_payment_count: u64,
    pub live_time: Duration,
}

#[derive(Debug, Clone)]
pub struct StoredRecord {
    pub address: NetworkAddress,
    pub record_kind: RecordKind,
    pub data_size: usize,
    pub payment: Option<SimulatedPayment>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct SimulatedPayment {
    pub payees: Vec<PeerId>,
    pub data_type: DataTypes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreResult {
    Stored,
    PaymentNotForUs,
    PayeesOutOfRange,
    DistanceTooFar,
}

/// Accumulator for tracking replication sources - requires majority consensus
/// Production requires 3+ peers (CLOSE_GROUP_SIZE / 2) reporting same record before accepting
pub struct ReplicationAccumulator {
    pub pending: HashMap<NetworkAddress, HashSet<PeerId>>,
}

impl ReplicationAccumulator {
    pub fn new() -> Self {
        Self {
            pending: HashMap::new(),
        }
    }

    /// Add a peer report for a record address and check if majority threshold is reached
    pub fn add_and_check_majority(&mut self, addr: &NetworkAddress, peer: PeerId) -> bool {
        let peers = self.pending.entry(addr.clone()).or_default();
        peers.insert(peer);
        peers.len() >= REPLICATION_MAJORITY_THRESHOLD
    }

    /// Take record if majority threshold was reached, removing it from pending
    #[allow(dead_code)]
    pub fn take_if_majority(&mut self, addr: &NetworkAddress) -> Option<HashSet<PeerId>> {
        if self
            .pending
            .get(addr)
            .map(|p| p.len())
            .unwrap_or(0)
            >= REPLICATION_MAJORITY_THRESHOLD
        {
            self.pending.remove(addr)
        } else {
            None
        }
    }

    /// Get count of pending records that haven't reached majority
    pub fn pending_count(&self) -> usize {
        self.pending
            .values()
            .filter(|peers| peers.len() < REPLICATION_MAJORITY_THRESHOLD)
            .count()
    }
}

impl Default for ReplicationAccumulator {
    fn default() -> Self {
        Self::new()
    }
}
