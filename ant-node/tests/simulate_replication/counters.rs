// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! Global atomic counters for tracking simulation statistics.

use std::sync::atomic::{AtomicUsize, Ordering};

// ============================================================================
// Global Counters
// ============================================================================

// Replication candidate selection paths
pub static REPLICATION_PEERS_WITHIN_RESPONSIBLE_RANGE: AtomicUsize = AtomicUsize::new(0);
pub static REPLICATION_PEERS_FALLBACK_TO_CLOSEST_K: AtomicUsize = AtomicUsize::new(0);

// is_in_range decision paths (simplified - matches production)
pub static REPLICATION_IN_RANGE: AtomicUsize = AtomicUsize::new(0);
pub static REPLICATION_OUT_OF_RANGE: AtomicUsize = AtomicUsize::new(0);

// Payment validation paths during upload
pub static UPLOAD_PAYMENT_ALL_PAYEES_IN_K_CLOSEST: AtomicUsize = AtomicUsize::new(0);
pub static UPLOAD_PAYMENT_VALIDATED_VIA_RESPONSIBLE_RANGE: AtomicUsize = AtomicUsize::new(0);
pub static UPLOAD_PAYMENT_NO_RESPONSIBLE_RANGE_TRUSTED: AtomicUsize = AtomicUsize::new(0);

// Replication message acceptance/rejection
pub static REPLICATION_MSG_ACCEPTED_FROM_K_CLOSEST: AtomicUsize = AtomicUsize::new(0);
pub static REPLICATION_MSG_REJECTED_NOT_IN_K_CLOSEST: AtomicUsize = AtomicUsize::new(0);

// Majority accumulation (production requires 2+ peers reporting same record)
pub static REPLICATION_MAJORITY_REACHED: AtomicUsize = AtomicUsize::new(0);
pub static REPLICATION_PENDING_MAJORITY: AtomicUsize = AtomicUsize::new(0);

/// Helper to load a counter value
pub fn load(counter: &AtomicUsize) -> usize {
    counter.load(Ordering::Relaxed)
}

/// Helper to increment a counter
pub fn increment(counter: &AtomicUsize) {
    counter.fetch_add(1, Ordering::Relaxed);
}

/// Helper to add a value to a counter
pub fn add(counter: &AtomicUsize, val: usize) {
    counter.fetch_add(val, Ordering::Relaxed);
}

/// Reset all counters to zero (for Monte Carlo trials)
pub fn reset_counters() {
    REPLICATION_PEERS_WITHIN_RESPONSIBLE_RANGE.store(0, Ordering::Relaxed);
    REPLICATION_PEERS_FALLBACK_TO_CLOSEST_K.store(0, Ordering::Relaxed);
    REPLICATION_IN_RANGE.store(0, Ordering::Relaxed);
    REPLICATION_OUT_OF_RANGE.store(0, Ordering::Relaxed);
    UPLOAD_PAYMENT_ALL_PAYEES_IN_K_CLOSEST.store(0, Ordering::Relaxed);
    UPLOAD_PAYMENT_VALIDATED_VIA_RESPONSIBLE_RANGE.store(0, Ordering::Relaxed);
    UPLOAD_PAYMENT_NO_RESPONSIBLE_RANGE_TRUSTED.store(0, Ordering::Relaxed);
    REPLICATION_MSG_ACCEPTED_FROM_K_CLOSEST.store(0, Ordering::Relaxed);
    REPLICATION_MSG_REJECTED_NOT_IN_K_CLOSEST.store(0, Ordering::Relaxed);
    REPLICATION_MAJORITY_REACHED.store(0, Ordering::Relaxed);
    REPLICATION_PENDING_MAJORITY.store(0, Ordering::Relaxed);
}
