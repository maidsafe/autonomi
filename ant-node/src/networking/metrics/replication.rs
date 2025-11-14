// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use prometheus_client::encoding::EncodeLabelSet;
use prometheus_client::encoding::EncodeLabelValue;
use std::collections::VecDeque;

#[derive(Debug, Clone, Hash, PartialEq, Eq, EncodeLabelSet)]
pub(crate) struct ReplicateCandidateLabels {
    pub(crate) range: Range,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, EncodeLabelValue)]
pub(crate) enum Range {
    WithinResponsibleDistance,
    OutsideResponsibleDistance,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, EncodeLabelSet)]
pub(crate) struct ReplicationSenderRangeLabels {
    pub(crate) in_range: bool,
    pub(crate) network_under_load: bool,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, EncodeLabelSet)]
pub(crate) struct IncomingKeysMetricLabels {
    pub(crate) metric_type: IncomingMetricType,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, EncodeLabelValue)]
pub(crate) enum IncomingMetricType {
    NewKeysPercent,
    OutOfRangeKeysPercent,
}

const STATS_WINDOW_SIZE: usize = 50;

/// Sliding window for tracking replication statistics
pub(crate) struct ReplicationStatsWindow {
    // Stores (total_keys, new_keys, out_of_range_keys) for last N requests
    window: VecDeque<(usize, usize, usize)>,
}

impl ReplicationStatsWindow {
    pub(crate) fn new() -> Self {
        Self {
            window: VecDeque::with_capacity(STATS_WINDOW_SIZE),
        }
    }

    /// Add a new entry to the sliding window
    pub(crate) fn add_entry(
        &mut self,
        total_keys: usize,
        new_keys: usize,
        out_of_range_keys: usize,
    ) {
        if self.window.len() >= STATS_WINDOW_SIZE {
            let _ = self.window.pop_front();
        }
        self.window
            .push_back((total_keys, new_keys, out_of_range_keys));
    }

    /// Calculate the percentage of new keys across the window
    pub(crate) fn calculate_new_keys_percent(&self) -> f64 {
        if self.window.is_empty() {
            return 0.0;
        }

        let (total_keys, total_new_keys) = self.window.iter().fold(
            (0usize, 0usize),
            |(acc_total, acc_new), &(total, new, _)| (acc_total + total, acc_new + new),
        );

        if total_keys == 0 {
            return 0.0;
        }

        (total_new_keys as f64 / total_keys as f64) * 100.0
    }

    /// Calculate the percentage of out-of-range keys across the window
    pub(crate) fn calculate_out_of_range_percent(&self) -> f64 {
        if self.window.is_empty() {
            return 0.0;
        }

        let (total_keys, total_out_of_range) = self.window.iter().fold(
            (0usize, 0usize),
            |(acc_total, acc_oor), &(total, _, oor)| (acc_total + total, acc_oor + oor),
        );

        if total_keys == 0 {
            return 0.0;
        }

        (total_out_of_range as f64 / total_keys as f64) * 100.0
    }
}
