// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! SimulatedNode implementation with all network operations.

use ant_protocol::{CLOSE_GROUP_SIZE, NetworkAddress};
use libp2p::{PeerId, kad::{KBucketDistance as Distance, U256}};
use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::Ordering;
use std::time::Duration;

use crate::config::{LIBP2P_K_VALUE, MAX_RECORDS_COUNT};
use crate::counters::*;
use crate::types::{SimulatedNode, SimulatedPayment, StoredRecord, StoreResult};

impl SimulatedNode {
    pub fn new(peer_id: PeerId) -> Self {
        Self {
            peer_id,
            address: NetworkAddress::from(peer_id),
            routing_table: BTreeMap::new(),
            stored_records: HashMap::new(),
            responsible_distance: None,
            network_size_estimate: 0,
            received_payment_count: 0,
            live_time: Duration::from_secs(0),
        }
    }

    /// Estimate network size based on routing table
    pub fn estimate_network_size(&mut self) {
        let (peers_in_non_full_buckets, num_of_full_buckets) =
            self.routing_table
                .values()
                .fold((0, 0), |(peers, full_buckets), bucket| {
                    if bucket.len() >= LIBP2P_K_VALUE {
                        (peers, full_buckets + 1)
                    } else {
                        (peers + bucket.len(), full_buckets)
                    }
                });

        self.network_size_estimate =
            (peers_in_non_full_buckets + 1) * 2_usize.pow(num_of_full_buckets as u32);
    }

    /// Get all peers from routing table
    pub fn all_routing_table_peers(&self, include_self: bool) -> Vec<PeerId> {
        let iter = self
            .routing_table
            .values()
            .flat_map(|peers| peers.iter())
            .filter(|&&peer| peer != self.peer_id);

        if include_self {
            std::iter::once(self.peer_id).chain(iter.copied()).collect()
        } else {
            iter.copied().collect()
        }
    }

    /// Find closest peers from local routing table
    pub fn find_closest_local(
        &self,
        target: &NetworkAddress,
        k: usize,
        include_self: bool,
    ) -> Vec<PeerId> {
        let mut peers: Vec<_> = self
            .all_routing_table_peers(include_self)
            .into_iter()
            .map(|peer| {
                let addr = NetworkAddress::from(peer);
                (peer, target.distance(&addr))
            })
            .collect();

        peers.sort_by_key(|(_, dist)| *dist);
        peers.into_iter().take(k).map(|(peer, _)| peer).collect()
    }

    /// Calculate responsible distance range
    pub fn calculate_responsible_distance(&mut self) {
        if self.network_size_estimate <= CLOSE_GROUP_SIZE {
            return;
        }

        let density = U256::MAX / U256::from(self.network_size_estimate);
        let density_distance = density * U256::from(CLOSE_GROUP_SIZE);

        let closest_k_peers = self.find_closest_local(
            &self.address.clone(),
            LIBP2P_K_VALUE,
            true,
        );

        if closest_k_peers.len() <= CLOSE_GROUP_SIZE + 2 {
            return;
        }

        let self_addr = NetworkAddress::from(self.peer_id);
        let close_peers_distance =
            self_addr.distance(&NetworkAddress::from(closest_k_peers[CLOSE_GROUP_SIZE + 1]));

        let distance = std::cmp::max(Distance(density_distance), close_peers_distance);
        self.responsible_distance = Some(distance);
    }

    /// Generate a quote for storing data
    pub fn generate_quote(&self, data_size: usize) -> u64 {
        let records_stored = self.stored_records.len();

        let close_records_stored = if let Some(resp_dist) = self.responsible_distance {
            self.stored_records
                .keys()
                .filter(|addr| self.address.distance(addr) <= resp_dist)
                .count()
        } else {
            records_stored
        };

        let utilization_ratio = close_records_stored as f64 / MAX_RECORDS_COUNT as f64;
        let base_cost = 1000u64;

        let utilization_multiplier = if utilization_ratio < 0.5 {
            1.0
        } else if utilization_ratio < 0.75 {
            2.0
        } else if utilization_ratio < 0.9 {
            5.0
        } else {
            10.0
        };

        let payment_multiplier = 1.0 + (self.received_payment_count as f64 * 0.1);
        let size_cost = (data_size as f64 / 1024.0 / 1024.0) * 100.0;

        let live_time_discount = if self.live_time.as_secs() > 3600 {
            0.9
        } else {
            1.0
        };

        let final_cost = (base_cost as f64
            + size_cost * utilization_multiplier * payment_multiplier * live_time_discount)
            as u64;

        final_cost.max(base_cost)
    }

    /// Check if this node should store a record based on distance
    pub fn should_store(&self, record_addr: &NetworkAddress) -> bool {
        if self.stored_records.len() < MAX_RECORDS_COUNT {
            if let Some(resp_dist) = self.responsible_distance {
                return self.address.distance(record_addr) <= resp_dist;
            }
            return true;
        }

        if let Some(farthest_dist) = self.get_farthest_record_distance() {
            return self.address.distance(record_addr) < farthest_dist;
        }

        false
    }

    /// Get distance to farthest stored record
    pub fn get_farthest_record_distance(&self) -> Option<Distance> {
        self.stored_records
            .keys()
            .map(|addr| self.address.distance(addr))
            .max()
    }

    /// Validate payment according to production logic
    pub fn validate_payment(
        &self,
        record_addr: &NetworkAddress,
        payment: &SimulatedPayment,
    ) -> Result<(), StoreResult> {
        if !payment.payees.contains(&self.peer_id) {
            return Err(StoreResult::PaymentNotForUs);
        }

        let closest_k = self.find_closest_local(
            record_addr,
            LIBP2P_K_VALUE,
            false,
        );

        let mut out_of_k_payees: Vec<PeerId> = payment
            .payees
            .iter()
            .filter(|p| !closest_k.contains(p))
            .cloned()
            .collect();

        if out_of_k_payees.is_empty() {
            UPLOAD_PAYMENT_ALL_PAYEES_IN_K_CLOSEST.fetch_add(1, Ordering::Relaxed);
            return Ok(());
        }

        if let Some(resp_dist) = self.responsible_distance {
            out_of_k_payees.retain(|peer_id| {
                let peer_addr = NetworkAddress::from(*peer_id);
                record_addr.distance(&peer_addr) > resp_dist
            });

            if !out_of_k_payees.is_empty() {
                return Err(StoreResult::PayeesOutOfRange);
            }

            UPLOAD_PAYMENT_VALIDATED_VIA_RESPONSIBLE_RANGE.fetch_add(1, Ordering::Relaxed);
            return Ok(());
        }

        UPLOAD_PAYMENT_NO_RESPONSIBLE_RANGE_TRUSTED.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    /// Store a record, returns detailed result about success or failure reason
    pub fn store_record(&mut self, record: StoredRecord) -> StoreResult {
        if let Some(ref payment) = record.payment {
            if let Err(reason) = self.validate_payment(&record.address, payment) {
                return reason;
            }
            self.received_payment_count += 1;
        }

        if self.should_store(&record.address) {
            self.stored_records.insert(record.address.clone(), record);
            return StoreResult::Stored;
        }
        StoreResult::DistanceTooFar
    }

    /// Check if record is in range for replication (matches production logic)
    pub fn is_in_range(&self, record_addr: &NetworkAddress, closest_peers: &[PeerId]) -> bool {
        let self_address = &self.address;

        let mut peers_with_self: Vec<_> = closest_peers.to_vec();
        peers_with_self.push(self.peer_id);
        peers_with_self.sort_by_key(|peer| self_address.distance(&NetworkAddress::from(*peer)));

        let farthest_distance = peers_with_self
            .last()
            .map(|peer| self_address.distance(&NetworkAddress::from(*peer)));

        if let Some(max_distance) = farthest_distance {
            let record_distance = self_address.distance(record_addr);
            let is_in_range = record_distance <= max_distance;

            if is_in_range {
                REPLICATION_IN_RANGE.fetch_add(1, Ordering::Relaxed);
            } else {
                REPLICATION_OUT_OF_RANGE.fetch_add(1, Ordering::Relaxed);
            }

            is_in_range
        } else {
            false
        }
    }

    /// Get replicate candidates with responsible_distance filtering
    pub fn get_replicate_candidates(&self, target: &NetworkAddress, is_periodic: bool) -> Vec<PeerId> {
        let expected_candidates = if is_periodic {
            CLOSE_GROUP_SIZE * 2
        } else {
            CLOSE_GROUP_SIZE
        };

        let closest_k_peers = self.find_closest_local(
            target,
            LIBP2P_K_VALUE,
            false,
        );

        if let Some(responsible_range) = self.responsible_distance {
            let peers_in_range: Vec<_> = closest_k_peers
                .iter()
                .filter(|peer_id| {
                    let peer_addr = NetworkAddress::from(**peer_id);
                    target.distance(&peer_addr) <= responsible_range
                })
                .copied()
                .collect();

            if peers_in_range.len() >= expected_candidates {
                REPLICATION_PEERS_WITHIN_RESPONSIBLE_RANGE.fetch_add(1, Ordering::Relaxed);
                return peers_in_range;
            }
        } else {
            tracing::error!("Node {} has no responsible distance set!", self.peer_id);
        }

        REPLICATION_PEERS_FALLBACK_TO_CLOSEST_K.fetch_add(1, Ordering::Relaxed);
        closest_k_peers
            .into_iter()
            .take(expected_candidates)
            .collect()
    }
}
