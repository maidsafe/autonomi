// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use super::Result;
use crate::networking::driver::{NETWORK_WIDE_REPLICATION_INTERVAL, NodeBehaviour};
use crate::networking::interface::NetworkEvent;
use ant_protocol::NetworkAddress;
use ant_protocol::storage::ValidationType;
use libp2p::Swarm;
use std::collections::VecDeque;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

/// The duration within which network-wide replication should be completed (4 days in seconds)
const REPLICATION_DEADLINE_SECS: u64 = 4 * 24 * 60 * 60;

/// The duration after which the network load flag is reset (72 hours in seconds)
const NETWORK_LOAD_TIMEOUT_SECS: u64 = 72 * 60 * 60;

pub(crate) struct NetworkWideReplication {
    last_record_count: usize,
    pending_records: VecDeque<(NetworkAddress, ValidationType)>,
    completed_records: Vec<(NetworkAddress, ValidationType)>,
    last_replication_time: Option<std::time::Instant>,
    complete_replication_within: std::time::Instant,
    event_sender: mpsc::Sender<NetworkEvent>,
    network_is_under_load: Option<std::time::Instant>,
}

impl NetworkWideReplication {
    pub(crate) fn new(event_sender: mpsc::Sender<NetworkEvent>) -> Self {
        let complete_replication_within =
            Instant::now() + Duration::from_secs(REPLICATION_DEADLINE_SECS);
        Self {
            pending_records: VecDeque::new(),
            completed_records: Vec::new(),
            last_record_count: 0,
            last_replication_time: None,
            complete_replication_within,
            event_sender,
            network_is_under_load: None,
        }
    }

    pub(crate) fn set_network_under_load(&mut self) {
        self.network_is_under_load = Some(Instant::now());
    }

    /// The network is under load if we performed network wide replication in the last 72 hours.
    pub(crate) fn is_network_under_load(&self) -> bool {
        self.network_is_under_load.is_some()
    }

    /// Calculate how many keys to send in this execution to complete replication within deadline.
    /// Returns `None` if we should skip this execution (not enough time passed or no pending keys).
    fn calculate_keys_to_send(
        pending_count: usize,
        time_remaining_secs: u64,
        elapsed_since_last: Option<Duration>,
        execute_interval_secs: u64,
    ) -> Option<usize> {
        if pending_count == 0 {
            return None;
        }

        // Calculate how many execute runs remain until deadline
        let execute_runs_remaining = if time_remaining_secs == 0 {
            0
        } else {
            (time_remaining_secs / execute_interval_secs).max(1)
        };

        // Calculate how many keys to send per execution
        if execute_runs_remaining == 0 {
            // Deadline passed, send all immediately
            Some(pending_count)
        } else {
            // Calculate keys per execution to finish within deadline
            let keys_per_execution =
                (pending_count as f64 / execute_runs_remaining as f64).ceil() as usize;

            // For low key counts with many executions remaining, check if we should send this time
            if keys_per_execution == 0 || (pending_count < execute_runs_remaining as usize) {
                // We have more executions than keys, so space them out
                // Calculate the interval between sends
                let target_interval_secs = time_remaining_secs / pending_count as u64;

                // Check if enough time has passed since last replication
                if let Some(elapsed) = elapsed_since_last {
                    if elapsed.as_secs() < target_interval_secs {
                        // Not enough time has passed, don't send yet
                        return None;
                    }
                    // Calculate how many we should send based on elapsed time
                    let sends = (elapsed.as_secs() / target_interval_secs).max(1) as usize;
                    Some(sends.min(pending_count))
                } else {
                    // First send
                    Some(1)
                }
            } else {
                // We have more keys than remaining executions, send multiple per execution
                Some(keys_per_execution.min(pending_count))
            }
        }
    }

    pub(crate) async fn execute(&mut self, swarm: &mut Swarm<NodeBehaviour>) -> Result<()> {
        // reset network load flag after 72 hours
        if let Some(last_time) = self.network_is_under_load
            && Instant::now().duration_since(last_time)
                > Duration::from_secs(NETWORK_LOAD_TIMEOUT_SECS)
        {
            self.network_is_under_load = None;
        }

        // add a new records to the queue
        let current_count = swarm.behaviour_mut().kademlia.store_mut().count();
        if self.last_record_count != current_count {
            self.last_record_count = current_count;
            let new_keys = swarm
                .behaviour_mut()
                .kademlia
                .store_mut()
                .record_addresses_ref()
                .iter()
                .map(|(_, (addr, validation_type, _))| (addr.clone(), validation_type.clone()))
                .filter(|key| {
                    !self.pending_records.contains(key) && !self.completed_records.contains(key)
                })
                .collect::<Vec<_>>();

            info!(
                "Adding {} new records to network wide replication queue",
                new_keys.len()
            );
            for key in new_keys {
                self.pending_records.push_back(key);
            }
        }

        let now = Instant::now();
        let pending_count = self.pending_records.len();

        // Calculate time remaining until deadline
        let time_remaining_secs = if self.complete_replication_within > now {
            self.complete_replication_within
                .duration_since(now)
                .as_secs()
        } else {
            0
        };

        // Calculate elapsed time since last replication
        let elapsed_since_last = self
            .last_replication_time
            .map(|last_time| now.duration_since(last_time));

        // Calculate how many keys to send
        let Some(keys_to_send) = Self::calculate_keys_to_send(
            pending_count,
            time_remaining_secs,
            elapsed_since_last,
            NETWORK_WIDE_REPLICATION_INTERVAL.as_secs(),
        ) else {
            // Not enough time has passed or no pending keys, skip this execution
            return Ok(());
        };

        info!(
            "Network wide replication: sending {keys_to_send} keys out of {pending_count} pending",
        );

        let mut keys_list = Vec::with_capacity(keys_to_send);
        for _i in 0..keys_to_send {
            if let Some(key) = self.pending_records.pop_front() {
                self.completed_records.push(key.clone());
                keys_list.push(key);
            }
        }
        // Send event to trigger replication
        let event = NetworkEvent::NetworkWideReplication { keys: keys_list };
        if let Err(err) = self.event_sender.send(event).await {
            warn!("Failed to send NetworkWideReplication event: {err}");
        } else {
            self.last_replication_time = Some(now);
        }

        // Check if we've completed the 7-day cycle (deadline passed and no pending records)
        if time_remaining_secs == 0 && self.pending_records.is_empty() {
            info!(
                "Network wide replication cycle completed. Completed {} records. Starting new 7-day cycle.",
                self.completed_records.len()
            );
            // Reset for a new 7-day cycle
            self.completed_records.clear();
            self.complete_replication_within = now + Duration::from_secs(REPLICATION_DEADLINE_SECS);
            self.last_replication_time = None;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXECUTE_INTERVAL_SECS: u64 = NETWORK_WIDE_REPLICATION_INTERVAL.as_secs(); // 15 minutes

    // Boundary/Invalid Input Tests
    #[test]
    fn keys_to_send_should_be_none_when_no_pending_keys() {
        let result = NetworkWideReplication::calculate_keys_to_send(
            0,
            REPLICATION_DEADLINE_SECS,
            None,
            EXECUTE_INTERVAL_SECS,
        );
        assert_eq!(result, None);
    }

    #[test]
    fn keys_to_send_should_be_all_pending_when_deadline_passed() {
        let result = NetworkWideReplication::calculate_keys_to_send(
            100,
            0, // deadline passed
            Some(Duration::from_secs(100)),
            EXECUTE_INTERVAL_SECS,
        );
        assert_eq!(result, Some(100));
    }

    // High Key Count Tests - More keys than executions
    #[test]
    fn keys_to_send_should_be_10_when_100_keys_with_10_runs_remaining() {
        let result = NetworkWideReplication::calculate_keys_to_send(
            100,
            10 * EXECUTE_INTERVAL_SECS,
            None,
            EXECUTE_INTERVAL_SECS,
        );
        assert_eq!(result, Some(10));
    }

    #[test]
    fn keys_to_send_should_ceil_when_keys_dont_divide_evenly() {
        // 100 keys / 7 runs = 14.28... -> should ceil to 15
        let result = NetworkWideReplication::calculate_keys_to_send(
            100,
            7 * EXECUTE_INTERVAL_SECS,
            None,
            EXECUTE_INTERVAL_SECS,
        );
        assert_eq!(result, Some(15));
    }

    #[test]
    fn keys_to_send_should_be_all_pending_when_only_one_run_remaining() {
        let result = NetworkWideReplication::calculate_keys_to_send(
            50,
            EXECUTE_INTERVAL_SECS,
            None,
            EXECUTE_INTERVAL_SECS,
        );
        assert_eq!(result, Some(50));
    }

    // Low Key Count Tests - More executions than keys
    #[test]
    fn keys_to_send_should_be_1_during_first_send() {
        let result = NetworkWideReplication::calculate_keys_to_send(
            7,
            REPLICATION_DEADLINE_SECS,
            None, // first send
            EXECUTE_INTERVAL_SECS,
        );
        assert_eq!(result, Some(1));
    }

    #[test]
    fn keys_to_send_should_be_1_when_exactly_one_interval_elapsed() {
        // 7 keys over 7 days = target interval of 24 hours (86400 secs)
        let target_interval = REPLICATION_DEADLINE_SECS / 7;
        let result = NetworkWideReplication::calculate_keys_to_send(
            7,
            REPLICATION_DEADLINE_SECS,
            Some(Duration::from_secs(target_interval)), // exactly 1 interval
            EXECUTE_INTERVAL_SECS,
        );
        assert_eq!(result, Some(1));
    }

    #[test]
    fn keys_to_send_should_be_none_when_not_enough_time_elapsed() {
        // 7 keys over 7 days = target interval of 24 hours (86400 secs)
        let target_interval = REPLICATION_DEADLINE_SECS / 7;
        let result = NetworkWideReplication::calculate_keys_to_send(
            7,
            REPLICATION_DEADLINE_SECS,
            Some(Duration::from_secs(target_interval / 2)), // only half interval
            EXECUTE_INTERVAL_SECS,
        );
        assert_eq!(result, None);
    }

    #[test]
    fn keys_to_send_should_be_3_when_three_intervals_elapsed() {
        // 7 keys over 7 days = target interval of 24 hours (86400 secs)
        let target_interval = REPLICATION_DEADLINE_SECS / 7;
        let result = NetworkWideReplication::calculate_keys_to_send(
            7,
            REPLICATION_DEADLINE_SECS,
            Some(Duration::from_secs(target_interval * 3)), // 3 intervals
            EXECUTE_INTERVAL_SECS,
        );
        assert_eq!(result, Some(3));
    }

    // Edge Cases
    #[test]
    fn keys_to_send_should_be_1_when_exactly_one_key_per_execution() {
        let result = NetworkWideReplication::calculate_keys_to_send(
            100,
            100 * EXECUTE_INTERVAL_SECS,
            None,
            EXECUTE_INTERVAL_SECS,
        );
        assert_eq!(result, Some(1));
    }

    #[test]
    fn keys_to_send_should_be_all_when_very_short_time_remaining() {
        // 1000 keys but only 1 execute interval remaining
        let result = NetworkWideReplication::calculate_keys_to_send(
            1000,
            EXECUTE_INTERVAL_SECS,
            None,
            EXECUTE_INTERVAL_SECS,
        );
        assert_eq!(result, Some(1000));
    }

    #[test]
    fn keys_to_send_should_cap_at_pending_count_when_many_intervals_passed() {
        // Only 5 keys pending but 100 intervals passed
        let target_interval = REPLICATION_DEADLINE_SECS / 5;
        let result = NetworkWideReplication::calculate_keys_to_send(
            5,
            REPLICATION_DEADLINE_SECS,
            Some(Duration::from_secs(target_interval * 100)),
            EXECUTE_INTERVAL_SECS,
        );
        assert_eq!(result, Some(5)); // capped at 5, not 100
    }
}
