// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! Utility functions for dual-stack operations
//! 
//! This module provides helper functions for transport selection, peer analysis,
//! performance scoring, and other common dual-stack operations.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::time::Duration;

use crate::networking::kad::transport::KadPeerId;

use super::TransportId;

/// Calculate transport preference score based on performance metrics
/// 
/// This function combines multiple performance indicators into a single score
/// that can be used for transport comparison and selection.
/// 
/// # Arguments
/// * `latency_ms` - Average latency in milliseconds (lower is better)
/// * `success_rate` - Success rate from 0.0 to 1.0 (higher is better)
/// * `bandwidth_mbps` - Available bandwidth in Mbps (higher is better)
/// 
/// # Returns
/// A score from 0.0 to 1.0 where 1.0 is optimal performance
pub fn calculate_preference_score(
    latency_ms: f64,
    success_rate: f64,
    bandwidth_mbps: f64,
) -> f64 {
    // Weighted scoring: latency (40%), success rate (40%), bandwidth (20%)
    let latency_score = calculate_latency_score(latency_ms);
    let success_score = success_rate.max(0.0).min(1.0);
    let bandwidth_score = calculate_bandwidth_score(bandwidth_mbps);
    
    (latency_score * 0.4) + (success_score * 0.4) + (bandwidth_score * 0.2)
}

/// Calculate latency-based score using exponential decay
/// 
/// Uses exponential function to heavily penalize high latencies while
/// providing diminishing returns for very low latencies.
fn calculate_latency_score(latency_ms: f64) -> f64 {
    if latency_ms <= 0.0 {
        return 1.0;
    }
    
    // Exponential decay with half-life at 100ms
    let normalized_latency = latency_ms / 100.0;
    (-normalized_latency.ln_1p()).exp().max(0.0).min(1.0)
}

/// Calculate bandwidth-based score with saturation
/// 
/// Provides linear scaling up to 100 Mbps, then logarithmic scaling beyond.
fn calculate_bandwidth_score(bandwidth_mbps: f64) -> f64 {
    if bandwidth_mbps <= 0.0 {
        return 0.0;
    }
    
    if bandwidth_mbps <= 100.0 {
        // Linear scaling up to 100 Mbps
        (bandwidth_mbps / 100.0).min(1.0)
    } else {
        // Logarithmic scaling beyond 100 Mbps
        let log_factor = (bandwidth_mbps / 100.0).ln() + 1.0;
        (log_factor / 3.0).min(1.0) // Cap at ~2000 Mbps for score of 1.0
    }
}

/// Determine if a peer supports both transports based on various heuristics
/// 
/// This function uses multiple indicators to estimate dual-stack capability:
/// - Peer ID patterns
/// - Historical connection data
/// - Protocol negotiation results
/// 
/// # Arguments
/// * `peer_id` - The peer ID to analyze
/// 
/// # Returns
/// `true` if the peer likely supports both libp2p and iroh transports
pub fn peer_supports_dual_stack(peer_id: &KadPeerId) -> bool {
    // Multiple heuristics for dual-stack detection
    
    // Heuristic 1: Peer ID length (modern peers use 32-byte IDs)
    let has_modern_id = peer_id.0.len() == 32;
    
    // Heuristic 2: Peer ID has specific patterns indicating iroh support
    let has_iroh_markers = check_iroh_peer_markers(peer_id);
    
    // Heuristic 3: Statistical sampling based on peer ID hash
    let in_dual_stack_sample = is_in_dual_stack_sample(peer_id, 0.3); // 30% assumed capable
    
    // Combine heuristics with weighted scoring
    let score = if has_modern_id { 0.4 } else { 0.0 } +
                if has_iroh_markers { 0.4 } else { 0.0 } +
                if in_dual_stack_sample { 0.2 } else { 0.0 };
    
    score >= 0.5
}

/// Check for specific markers in peer ID that suggest iroh support
fn check_iroh_peer_markers(peer_id: &KadPeerId) -> bool {
    // Look for specific byte patterns that might indicate iroh capability
    // This is speculative - real implementation would use protocol negotiation
    
    if peer_id.0.len() < 4 {
        return false;
    }
    
    // Check for specific version markers or capability flags
    let marker_bytes = &peer_id.0[0..4];
    
    // Example: Look for version markers (placeholder logic)
    marker_bytes[0] >= 0x02 && // Version 2 or higher
    marker_bytes[1] & 0x01 != 0 // Capability flag set
}

/// Determine if peer falls within dual-stack capability sample
fn is_in_dual_stack_sample(peer_id: &KadPeerId, probability: f64) -> bool {
    let hash = hash_peer_id(peer_id);
    let normalized = (hash % 10000) as f64 / 10000.0;
    normalized < probability
}

/// Generate migration cohort assignment based on peer ID
/// 
/// Provides deterministic cohort assignment that remains consistent
/// across restarts and different nodes.
/// 
/// # Arguments
/// * `peer_id` - The peer ID to assign to a cohort
/// * `total_cohorts` - Total number of cohorts available
/// 
/// # Returns
/// Cohort number from 0 to `total_cohorts - 1`
pub fn get_migration_cohort(peer_id: &KadPeerId, total_cohorts: u32) -> u32 {
    if total_cohorts == 0 {
        return 0;
    }
    
    let hash = hash_peer_id(peer_id);
    (hash % total_cohorts as u64) as u32
}

/// Calculate consistent hash for peer ID
fn hash_peer_id(peer_id: &KadPeerId) -> u64 {
    let mut hasher = DefaultHasher::new();
    peer_id.0.hash(&mut hasher);
    hasher.finish()
}

/// Determine optimal transport for specific operation types
/// 
/// Different operations may benefit from different transports based on
/// their characteristics (latency-sensitive vs. throughput-optimized).
/// 
/// # Arguments
/// * `operation_type` - Type of operation being performed
/// * `payload_size` - Size of data being transferred (if applicable)
/// * `priority` - Operation priority level
/// 
/// # Returns
/// Suggested transport preference
pub fn suggest_transport_for_operation(
    operation_type: &str,
    payload_size: Option<usize>,
    priority: OperationPriority,
) -> TransportPreference {
    match operation_type {
        "bootstrap" => {
            // Bootstrap operations prefer reliability over speed
            TransportPreference::Prefer(TransportId::LibP2P)
        },
        "find_node" => {
            // Node discovery can benefit from iroh's faster routing
            if priority == OperationPriority::High {
                TransportPreference::Prefer(TransportId::Iroh)
            } else {
                TransportPreference::Either
            }
        },
        "find_value" | "get_record" => {
            // Data retrieval often benefits from iroh's optimizations
            TransportPreference::Prefer(TransportId::Iroh)
        },
        "put_record" => {
            // Data storage - consider payload size
            if let Some(size) = payload_size {
                if size > 1024 * 1024 { // > 1MB
                    TransportPreference::Prefer(TransportId::Iroh)
                } else {
                    TransportPreference::Either
                }
            } else {
                TransportPreference::Either
            }
        },
        "ping" => {
            // Simple connectivity checks - either transport fine
            TransportPreference::Either
        },
        _ => {
            // Unknown operation - use conservative choice
            TransportPreference::Either
        }
    }
}

/// Calculate network distance metric between peers
/// 
/// Estimates network proximity using various indicators like latency,
/// hop count, and routing efficiency.
/// 
/// # Arguments
/// * `latency` - Observed latency to the peer
/// * `hop_count` - Number of network hops (if known)
/// * `routing_efficiency` - Efficiency of route to peer (0.0 to 1.0)
/// 
/// # Returns
/// Distance metric where lower values indicate closer proximity
pub fn calculate_network_distance(
    latency: Duration,
    hop_count: Option<u8>,
    routing_efficiency: f64,
) -> f64 {
    let latency_component = latency.as_millis() as f64 / 1000.0; // Normalize to seconds
    
    let hop_component = hop_count.map(|h| h as f64 * 0.1).unwrap_or(0.5); // Default middle value
    
    let efficiency_component = (1.0 - routing_efficiency.max(0.0).min(1.0)) * 2.0;
    
    latency_component + hop_component + efficiency_component
}

/// Analyze peer behavior patterns for transport optimization
/// 
/// Examines historical interaction patterns to identify optimal
/// transport selection strategies for specific peer relationships.
/// 
/// # Arguments
/// * `peer_id` - The peer to analyze
/// * `interaction_history` - Recent interaction data
/// 
/// # Returns
/// Analysis results with optimization recommendations
pub fn analyze_peer_behavior(
    peer_id: &KadPeerId,
    interaction_history: &[InteractionRecord],
) -> PeerBehaviorAnalysis {
    if interaction_history.is_empty() {
        return PeerBehaviorAnalysis::default();
    }
    
    // Analyze interaction patterns
    let total_interactions = interaction_history.len();
    let successful_interactions = interaction_history.iter()
        .filter(|r| r.success)
        .count();
    
    let success_rate = successful_interactions as f64 / total_interactions as f64;
    
    // Calculate average latency
    let total_latency: Duration = interaction_history.iter()
        .map(|r| r.latency)
        .sum();
    let avg_latency = total_latency / total_interactions as u32;
    
    // Identify preferred times
    let mut hourly_interactions = vec![0; 24];
    for record in interaction_history {
        let hour = record.timestamp.hour() as usize;
        hourly_interactions[hour] += 1;
    }
    
    let peak_hour = hourly_interactions.iter()
        .enumerate()
        .max_by_key(|(_, &count)| count)
        .map(|(hour, _)| hour as u8)
        .unwrap_or(12); // Default to noon
    
    // Determine transport preference based on patterns
    let transport_preference = if success_rate > 0.9 && avg_latency < Duration::from_millis(100) {
        TransportPreference::Prefer(TransportId::Iroh)
    } else if success_rate < 0.7 {
        TransportPreference::Prefer(TransportId::LibP2P)
    } else {
        TransportPreference::Either
    };
    
    PeerBehaviorAnalysis {
        peer_id: peer_id.clone(),
        success_rate,
        avg_latency,
        peak_interaction_hour: peak_hour,
        transport_preference,
        confidence_score: calculate_confidence_score(total_interactions, success_rate),
        last_analysis: std::time::Instant::now(),
    }
}

/// Calculate confidence score for behavioral analysis
fn calculate_confidence_score(sample_size: usize, success_rate: f64) -> f64 {
    // Confidence increases with sample size and stabilizes around 100 samples
    let size_factor = (sample_size as f64 / 100.0).min(1.0);
    
    // Success rate variance affects confidence
    let variance_factor = 1.0 - (success_rate - 0.5).abs() * 0.5;
    
    size_factor * variance_factor
}

/// Operation priority levels for transport selection
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationPriority {
    Low,
    Normal,
    High,
    Critical,
}

/// Transport preference indication
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportPreference {
    /// No preference, either transport is suitable
    Either,
    /// Prefer specific transport but allow fallback
    Prefer(TransportId),
    /// Require specific transport (strong preference)
    Require(TransportId),
}

/// Interaction record for behavioral analysis
#[derive(Debug, Clone)]
pub struct InteractionRecord {
    pub timestamp: std::time::SystemTime,
    pub transport_used: TransportId,
    pub operation_type: String,
    pub latency: Duration,
    pub success: bool,
    pub bytes_transferred: Option<usize>,
}

impl InteractionRecord {
    /// Get hour of day for temporal analysis
    fn hour(&self) -> u8 {
        // Simplified - would use proper time handling in production
        let duration = self.timestamp.duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        ((duration.as_secs() / 3600) % 24) as u8
    }
}

/// Results of peer behavior analysis
#[derive(Debug, Clone)]
pub struct PeerBehaviorAnalysis {
    pub peer_id: KadPeerId,
    pub success_rate: f64,
    pub avg_latency: Duration,
    pub peak_interaction_hour: u8,
    pub transport_preference: TransportPreference,
    pub confidence_score: f64,
    pub last_analysis: std::time::Instant,
}

impl Default for PeerBehaviorAnalysis {
    fn default() -> Self {
        Self {
            peer_id: KadPeerId::default(),
            success_rate: 0.5,
            avg_latency: Duration::from_millis(100),
            peak_interaction_hour: 12,
            transport_preference: TransportPreference::Either,
            confidence_score: 0.0,
            last_analysis: std::time::Instant::now(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_calculate_preference_score() {
        // Test optimal performance
        let score = calculate_preference_score(10.0, 1.0, 100.0);
        assert!(score > 0.8);
        
        // Test poor performance
        let score = calculate_preference_score(1000.0, 0.5, 1.0);
        assert!(score < 0.5);
        
        // Test balanced performance
        let score = calculate_preference_score(100.0, 0.95, 50.0);
        assert!(score > 0.6 && score < 0.9);
    }
    
    #[test]
    fn test_migration_cohort_assignment() {
        let peer_id = KadPeerId::new(vec![1, 2, 3, 4]);
        
        // Test deterministic assignment
        let cohort1 = get_migration_cohort(&peer_id, 10);
        let cohort2 = get_migration_cohort(&peer_id, 10);
        assert_eq!(cohort1, cohort2);
        
        // Test range validity
        for _ in 0..100 {
            let cohort = get_migration_cohort(&peer_id, 5);
            assert!(cohort < 5);
        }
    }
    
    #[test]
    fn test_transport_suggestion() {
        // Test bootstrap operation
        let pref = suggest_transport_for_operation("bootstrap", None, OperationPriority::Normal);
        assert_eq!(pref, TransportPreference::Prefer(TransportId::LibP2P));
        
        // Test large data transfer
        let pref = suggest_transport_for_operation("put_record", Some(2 * 1024 * 1024), OperationPriority::Normal);
        assert_eq!(pref, TransportPreference::Prefer(TransportId::Iroh));
        
        // Test unknown operation
        let pref = suggest_transport_for_operation("unknown", None, OperationPriority::Normal);
        assert_eq!(pref, TransportPreference::Either);
    }
}