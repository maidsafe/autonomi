// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! Statistics helpers for tracking simulation results.

use crate::simulate_replication::export::CoverageExport;

// ============================================================================
// Statistics Helpers
// ============================================================================

pub struct UploadStats {
    pub total: usize,
    pub sum: usize,
    pub payment_not_for_us: usize,
    pub payees_out_of_range: usize,
    pub distance_too_far: usize,
}

impl UploadStats {
    pub fn new() -> Self {
        Self {
            total: 0,
            sum: 0,
            payment_not_for_us: 0,
            payees_out_of_range: 0,
            distance_too_far: 0,
        }
    }

    pub fn add_upload(
        &mut self,
        stored_count: usize,
        payment_not_for_us: usize,
        payees_out_of_range: usize,
        distance_too_far: usize,
    ) {
        self.total += 1;
        self.sum += stored_count;
        self.payment_not_for_us += payment_not_for_us;
        self.payees_out_of_range += payees_out_of_range;
        self.distance_too_far += distance_too_far;
    }

    pub fn average(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            self.sum as f64 / self.total as f64
        }
    }
}

impl Default for UploadStats {
    fn default() -> Self {
        Self::new()
    }
}

pub struct CoverageStats {
    pub total_chunks: usize,
    pub holders_7: usize,
    pub holders_6: usize,
    pub holders_5: usize,
    pub holders_4: usize,
    pub holders_3: usize,
    pub holders_2: usize,
    pub holders_1: usize,
    pub holders_0: usize,
}

impl CoverageStats {
    pub fn new() -> Self {
        Self {
            total_chunks: 0,
            holders_7: 0,
            holders_6: 0,
            holders_5: 0,
            holders_4: 0,
            holders_3: 0,
            holders_2: 0,
            holders_1: 0,
            holders_0: 0,
        }
    }

    pub fn add_chunk(&mut self, stored_count: usize) {
        self.total_chunks += 1;
        match stored_count {
            7 => self.holders_7 += 1,
            6 => self.holders_6 += 1,
            5 => self.holders_5 += 1,
            4 => self.holders_4 += 1,
            3 => self.holders_3 += 1,
            2 => self.holders_2 += 1,
            1 => self.holders_1 += 1,
            0 => self.holders_0 += 1,
            _ => {}
        }
    }

    pub fn percent(&self, count: usize) -> f64 {
        100.0 * count as f64 / self.total_chunks as f64
    }

    /// Generate ASCII bar for coverage visualization
    pub fn ascii_bar(&self, width: usize) -> String {
        let pct = self.average_percent() / 100.0;
        let filled = (pct * width as f64).round() as usize;
        let empty = width.saturating_sub(filled);
        format!("[{}{}]", "█".repeat(filled), "░".repeat(empty))
    }

    pub fn average_percent(&self) -> f64 {
        let total_coverage = (self.holders_7 * 7
            + self.holders_6 * 6
            + self.holders_5 * 5
            + self.holders_4 * 4
            + self.holders_3 * 3
            + self.holders_2 * 2
            + self.holders_1) as f64;
        let max_coverage = (self.total_chunks * 7) as f64;
        100.0 * total_coverage / max_coverage
    }

    pub fn to_export(&self) -> CoverageExport {
        CoverageExport {
            total_chunks: self.total_chunks,
            distribution: [
                self.holders_0,
                self.holders_1,
                self.holders_2,
                self.holders_3,
                self.holders_4,
                self.holders_5,
                self.holders_6,
                self.holders_7,
            ],
            average_percent: self.average_percent(),
        }
    }
}

impl Default for CoverageStats {
    fn default() -> Self {
        Self::new()
    }
}
