// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use super::{AnalysisErrorDisplay, ClosestPeerStatus, NetworkErrorDisplay};
use crate::commands::analyze::{HolderStatus, KAD_HOLDERS_QUERY_RANGE};
use autonomi::client::analyze::{Analysis, AnalysisError};
use autonomi::networking::NetworkAddress;
use color_eyre::eyre::Result;
use file_rotate::{ContentLimit, FileRotate, compression::Compression, suffix::AppendCount};
use serde::Serialize;
use std::io::Write;
use std::path::Path;
use std::time::SystemTime;

/// Root JSON output structure
#[derive(Debug, Serialize)]
pub struct JsonOutput {
    pub provided_address: String,
    pub ant_package_version: String,
    pub analyzed_addresses: Vec<AnalyzedAddress>,
}

/// Analysis result for a single address
#[derive(Debug, Serialize)]
pub struct AnalyzedAddress {
    pub target_address: String,
    pub kad_method: KadMethod,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub closest_method: Option<ClosestMethod>,
}

/// Kademlia query result
#[derive(Debug, Serialize)]
pub struct KadMethod {
    pub analysis_query: KadAnalysisQuery,
    pub holder_query: KadHolderQuery,
}

/// Kademlia analysis query result
#[derive(Debug, Serialize)]
pub struct KadAnalysisQuery {
    pub query_status: QueryStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_type: Option<AnalysisErrorDisplay>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address_type: Option<String>,
}

/// Kademlia holder query result
#[derive(Debug, Serialize)]
pub struct KadHolderQuery {
    pub query_range: u32,
    pub holding_count: u32,
    pub holders: Vec<KadHolders>,
}

#[derive(Debug, Serialize)]
pub struct KadHolders {
    pub peer_id: String,
    pub distance_to_target_ilog2: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub record_size_bytes: Option<usize>,
}

/// Closest peers query result
#[derive(Debug, Serialize)]
pub struct ClosestMethod {
    pub query_status: QueryStatus,
    pub closest_peers: Vec<ClosestPeer>,
    pub peer_count: usize,
    pub holders_among_7_closest: usize,
    pub holders_among_20_closest: usize,
}

/// Individual peer information
#[derive(Debug, Serialize)]
pub struct ClosestPeer {
    pub peer_id: String,
    pub holding_status: ClosestPeerHoldingStatus,
    pub distance_to_target_ilog2: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub record_size_bytes: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_type: Option<NetworkErrorDisplay>,
}

/// Status of the kademlia query
#[derive(Debug, Serialize)]
#[allow(dead_code)]
pub enum QueryStatus {
    Success,
    Error,
}

/// Holding status for individual peers in closest peers query
#[derive(Debug, Serialize)]
pub enum ClosestPeerHoldingStatus {
    Holding,
    NotHolding,
    FailedQuery,
}

impl JsonOutput {
    pub fn new(provided_address: String) -> Self {
        Self {
            provided_address,
            ant_package_version: env!("CARGO_PKG_VERSION").to_string(),
            analyzed_addresses: Vec::new(),
        }
    }

    pub fn add_address(&mut self, analyzed: AnalyzedAddress) {
        self.analyzed_addresses.push(analyzed);
    }
}

impl AnalyzedAddress {
    pub fn new(
        address: String,
        analysis_result: &Result<Analysis, AnalysisError>,
        closest_peers_data: Option<Vec<ClosestPeerStatus>>,
        holders: Option<Vec<HolderStatus>>,
        target_addr: &NetworkAddress,
    ) -> Self {
        let kad_method =
            KadMethod::from_analysis_and_holders(analysis_result, holders, target_addr);
        let closest_method =
            closest_peers_data.map(|peers| ClosestMethod::from_peer_statuses(peers, target_addr));
        Self {
            target_address: strip_0x_prefix(&address),
            kad_method,
            closest_method,
        }
    }
}

impl KadMethod {
    fn from_analysis_and_holders(
        analysis: &Result<Analysis, AnalysisError>,
        holders: Option<Vec<HolderStatus>>,
        target_addr: &NetworkAddress,
    ) -> Self {
        let analysis_query = KadAnalysisQuery::from_analysis(analysis);
        let holder_query = KadHolderQuery::from_holders(holders, target_addr);

        Self {
            analysis_query,
            holder_query,
        }
    }
}

impl KadAnalysisQuery {
    fn from_analysis(analysis: &Result<Analysis, AnalysisError>) -> Self {
        match analysis {
            Ok(analysis) => Self {
                query_status: QueryStatus::Success,
                error_type: None,
                address_type: Some(get_analysis_type(analysis)),
            },
            Err(err) => Self {
                query_status: QueryStatus::Error,
                error_type: Some(AnalysisErrorDisplay::from_analysis_error(err)),
                address_type: None,
            },
        }
    }
}

impl KadHolderQuery {
    fn from_holders(holders: Option<Vec<HolderStatus>>, target_addr: &NetworkAddress) -> Self {
        let kad_holders = if let Some(holders) = holders {
            holders
                .into_iter()
                .map(|holder| {
                    let peer_addr = NetworkAddress::from(holder.peer_id);
                    let distance = target_addr.distance(&peer_addr);
                    let distance_to_target_ilog2 = distance.ilog2().unwrap_or(0);
                    KadHolders {
                        peer_id: holder.peer_id.to_string(),
                        distance_to_target_ilog2,
                        record_size_bytes: Some(holder.size),
                    }
                })
                .collect()
        } else {
            Vec::new()
        };

        Self {
            query_range: KAD_HOLDERS_QUERY_RANGE,
            holding_count: kad_holders.len() as u32,
            holders: kad_holders,
        }
    }
}

impl ClosestMethod {
    fn from_peer_statuses(statuses: Vec<ClosestPeerStatus>, target_addr: &NetworkAddress) -> Self {
        let peer_count = statuses.len();

        // Check if any peer has a failed query
        let has_failed_query = statuses
            .iter()
            .any(|status| matches!(status, ClosestPeerStatus::FailedQuery { .. }));

        // Determine query status: success if all peers are error-free (Holding or NotHolding)
        // failure if even one peer had a FailedQuery
        let query_status = if has_failed_query {
            QueryStatus::Error
        } else {
            QueryStatus::Success
        };

        // Count holders among first 7 and all 20 peers
        let holders_among_7_closest = statuses
            .iter()
            .take(7)
            .filter(|status| matches!(status, ClosestPeerStatus::Holding { .. }))
            .count();

        let holders_among_20_closest = statuses
            .iter()
            .filter(|status| matches!(status, ClosestPeerStatus::Holding { .. }))
            .count();

        let closest_peers = statuses
            .into_iter()
            .map(|status| ClosestPeer::from_status(status, target_addr))
            .collect();

        Self {
            query_status,
            closest_peers,
            peer_count,
            holders_among_7_closest,
            holders_among_20_closest,
        }
    }
}

impl ClosestPeer {
    fn from_status(status: ClosestPeerStatus, target_addr: &NetworkAddress) -> Self {
        let peer_id = status.peer_id().to_string();
        let peer_addr = NetworkAddress::from(*status.peer_id());
        let distance = target_addr.distance(&peer_addr);
        let distance_ilog2 = distance.ilog2().unwrap_or(0);

        match status {
            ClosestPeerStatus::Holding { size, .. } => Self {
                peer_id,
                holding_status: ClosestPeerHoldingStatus::Holding,
                distance_to_target_ilog2: distance_ilog2,
                record_size_bytes: Some(size),
                error_type: None,
            },
            ClosestPeerStatus::NotHolding { .. } => Self {
                peer_id,
                holding_status: ClosestPeerHoldingStatus::NotHolding,
                distance_to_target_ilog2: distance_ilog2,
                record_size_bytes: None,
                error_type: None,
            },
            ClosestPeerStatus::FailedQuery { error, .. } => Self {
                peer_id,
                holding_status: ClosestPeerHoldingStatus::FailedQuery,
                distance_to_target_ilog2: distance_ilog2,
                record_size_bytes: None,
                error_type: Some(error),
            },
        }
    }
}

/// Get the type name from Analysis enum
fn get_analysis_type(analysis: &Analysis) -> String {
    match analysis {
        Analysis::Chunk(_) => "Chunk".to_string(),
        Analysis::GraphEntry(_) => "GraphEntry".to_string(),
        Analysis::Pointer(_) => "Pointer".to_string(),
        Analysis::Scratchpad(_) => "Scratchpad".to_string(),
        Analysis::Register { .. } => "Register".to_string(),
        Analysis::DataMap { .. } => "DataMap".to_string(),
        Analysis::RawDataMap { .. } => "RawDataMap".to_string(),
        Analysis::PublicArchive { .. } => "PublicArchive".to_string(),
        Analysis::PrivateArchive(_) => "PrivateArchive".to_string(),
    }
}

/// Remove 0x prefix from hex strings if present
fn strip_0x_prefix(s: &str) -> String {
    s.trim_start_matches("0x").to_string()
}

/// Flattened JSON entry for transformed output
#[derive(Debug, Serialize)]
pub struct FlattenedEntry {
    pub target_address: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peer: Option<usize>,
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peer_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub distance_to_target_ilog2: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub record_size_bytes: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub holding_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub holding_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub holders_among_closest: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address_type: Option<String>,
    pub timestamp: u128,
}

impl FlattenedEntry {
    /// Flatten holder_query entries
    fn from_holder_query(
        target_address: &str,
        peer_idx: usize,
        holder: &KadHolders,
        holding_count: u32,
        query_status: &str,
        address_type: Option<&str>,
        timestamp: u128,
    ) -> Self {
        Self {
            target_address: target_address.to_string(),
            peer: Some(peer_idx),
            source: "holder_query".to_string(),
            peer_id: Some(holder.peer_id.clone()),
            distance_to_target_ilog2: Some(holder.distance_to_target_ilog2),
            record_size_bytes: holder.record_size_bytes,
            holding_count: Some(holding_count),
            holding_status: None,
            holders_among_closest: None,
            query_status: Some(query_status.to_string()),
            address_type: address_type.map(|s| s.to_string()),
            timestamp,
        }
    }

    /// Flatten closest_peers entries
    fn from_closest_peer(
        target_address: &str,
        peer_idx: usize,
        peer: &ClosestPeer,
        query_status: &str,
        address_type: Option<&str>,
        timestamp: u128,
    ) -> Self {
        let holding_status = match &peer.holding_status {
            ClosestPeerHoldingStatus::Holding => "Holding",
            ClosestPeerHoldingStatus::NotHolding => "NotHolding",
            ClosestPeerHoldingStatus::FailedQuery => "FailedQuery",
        };

        Self {
            target_address: target_address.to_string(),
            peer: Some(peer_idx),
            source: "closest_peers".to_string(),
            peer_id: Some(peer.peer_id.clone()),
            distance_to_target_ilog2: Some(peer.distance_to_target_ilog2),
            record_size_bytes: peer.record_size_bytes,
            holding_count: None,
            holding_status: Some(holding_status.to_string()),
            holders_among_closest: None,
            query_status: Some(query_status.to_string()),
            address_type: address_type.map(|s| s.to_string()),
            timestamp,
        }
    }

    /// Flatten closest_7 or closest_20 summary
    fn from_closest_summary(
        target_address: &str,
        source: &str,
        holders_count: usize,
        query_status: &str,
        address_type: Option<&str>,
        timestamp: u128,
    ) -> Self {
        Self {
            target_address: target_address.to_string(),
            peer: None,
            source: source.to_string(),
            peer_id: None,
            distance_to_target_ilog2: None,
            record_size_bytes: None,
            holding_count: None,
            holding_status: None,
            holders_among_closest: Some(holders_count),
            query_status: Some(query_status.to_string()),
            address_type: address_type.map(|s| s.to_string()),
            timestamp,
        }
    }
}

const MAX_FILE_SIZE: usize = 50 * 1024 * 1024; // 50 MB
const MAX_FILES: usize = 10;

/// Writer for JSON output that supports both file and directory targets
pub struct JsonWriter {
    writer: WriterType,
    transformed_writer: WriterType,
    /// Current timestamp base for the current target address batch (in nanoseconds)
    current_timestamp_base_ns: u128,
}

enum WriterType {
    /// Simple file writer (append-only)
    File(std::fs::File),
    /// Rotating file writer for directories
    Rotating(FileRotate<AppendCount>),
}

impl JsonWriter {
    /// Create a new JSON writer
    ///
    /// If path is a file, opens it in append mode.
    /// If path is a directory, creates a rotating file appender with:
    /// - 50 MB max per file
    /// - 10 files max
    /// - No compression
    pub fn new(path: &Path) -> Result<Self> {
        let (writer, transformed_writer) = if path.is_dir() {
            // Directory: use rotating writers for both files
            let json_file_path = path.join("analyze.json");
            let file_rotate = FileRotate::new(
                json_file_path,
                AppendCount::new(MAX_FILES),
                ContentLimit::BytesSurpassed(MAX_FILE_SIZE),
                Compression::None,
                #[cfg(unix)]
                None,
            );
            
            let transformed_file_path = path.join("analyze_transformed.json");
            let transformed_rotate = FileRotate::new(
                transformed_file_path,
                AppendCount::new(MAX_FILES),
                ContentLimit::BytesSurpassed(MAX_FILE_SIZE),
                Compression::None,
                #[cfg(unix)]
                None,
            );
            
            (WriterType::Rotating(file_rotate), WriterType::Rotating(transformed_rotate))
        } else {
            // File: use simple append mode
            // Create parent directory if it doesn't exist
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            
            let file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)?;
            
            // Create transformed file path by inserting "_transformed" before extension
            let transformed_path = if let Some(parent) = path.parent() {
                let file_name = path.file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("analyze");
                let ext = path.extension()
                    .and_then(|s| s.to_str())
                    .unwrap_or("json");
                parent.join(format!("{file_name}_transformed.{ext}"))
            } else {
                let file_name = path.file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("analyze");
                let ext = path.extension()
                    .and_then(|s| s.to_str())
                    .unwrap_or("json");
                Path::new(&format!("{file_name}_transformed.{ext}")).to_path_buf()
            };
            
            let transformed_file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(transformed_path)?;
            
            (WriterType::File(file), WriterType::File(transformed_file))
        };

        // Initialize timestamp with current time in nanoseconds
        let current_timestamp_base_ns = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);

        Ok(Self { 
            writer,
            transformed_writer,
            current_timestamp_base_ns,
        })
    }

    /// Write JSON string to the original analyze.json output
    pub fn write_json(&mut self, json_str: &str) -> Result<()> {
        match &mut self.writer {
            WriterType::File(file) => {
                writeln!(file, "{json_str}")?;
                file.flush()?;
            }
            WriterType::Rotating(rotate) => {
                writeln!(rotate, "{json_str}")?;
                rotate.flush()?;
            }
        }
        Ok(())
    }

    /// Write the transformed JSON output for the given JsonOutput
    pub fn write_transformed_json(&mut self, json_output: &JsonOutput) -> Result<()> {
        // Process each analyzed address
        for analyzed_addr in &json_output.analyzed_addresses {
            let base_timestamp = self.current_timestamp_base_ns;
            let mut timestamp = base_timestamp;

            // Get query status and address type from kad_method
            let kad_query_status = match analyzed_addr.kad_method.analysis_query.query_status {
                QueryStatus::Success => "Success",
                QueryStatus::Error => "Error",
            };
            let kad_address_type = analyzed_addr.kad_method.analysis_query.address_type.as_deref();

            // Flatten holder_query peers
            for (idx, holder) in analyzed_addr.kad_method.holder_query.holders.iter().enumerate() {
                let entry = FlattenedEntry::from_holder_query(
                    &analyzed_addr.target_address,
                    idx,
                    holder,
                    analyzed_addr.kad_method.holder_query.holding_count,
                    kad_query_status,
                    kad_address_type,
                    timestamp,
                );
                self.write_flattened_entry(&entry)?;
                timestamp += 1;
            }

            // Process closest_method if present
            if let Some(closest_method) = &analyzed_addr.closest_method {
                let closest_query_status = match closest_method.query_status {
                    QueryStatus::Success => "Success",
                    QueryStatus::Error => "Error",
                };

                // Flatten closest_peers
                for (idx, peer) in closest_method.closest_peers.iter().enumerate() {
                    let entry = FlattenedEntry::from_closest_peer(
                        &analyzed_addr.target_address,
                        idx,
                        peer,
                        closest_query_status,
                        kad_address_type,
                        timestamp,
                    );
                    self.write_flattened_entry(&entry)?;
                    timestamp += 1;
                }

                // Flatten closest_7 summary (offset +0)
                let entry_7 = FlattenedEntry::from_closest_summary(
                    &analyzed_addr.target_address,
                    "closest_7",
                    closest_method.holders_among_7_closest,
                    closest_query_status,
                    kad_address_type,
                    base_timestamp,
                );
                self.write_flattened_entry(&entry_7)?;

                // Flatten closest_20 summary (offset +1)
                let entry_20 = FlattenedEntry::from_closest_summary(
                    &analyzed_addr.target_address,
                    "closest_20",
                    closest_method.holders_among_20_closest,
                    closest_query_status,
                    kad_address_type,
                    base_timestamp + 1,
                );
                self.write_flattened_entry(&entry_20)?;
            }

            // Increment base timestamp by 1ms for next target address
            self.current_timestamp_base_ns += 1_000_000;
        }

        Ok(())
    }

    /// Write a single flattened entry to the transformed output
    fn write_flattened_entry(&mut self, entry: &FlattenedEntry) -> Result<()> {
        let json_str = serde_json::to_string(entry)?;
        match &mut self.transformed_writer {
            WriterType::File(file) => {
                writeln!(file, "{json_str}")?;
                file.flush()?;
            }
            WriterType::Rotating(rotate) => {
                writeln!(rotate, "{json_str}")?;
                rotate.flush()?;
            }
        }
        Ok(())
    }
}
