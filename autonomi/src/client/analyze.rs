// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::{
    Bytes, Client, PublicKey,
    chunk::{Chunk, ChunkAddress, DataMapChunk},
    client::config::CHUNK_DOWNLOAD_BATCH_SIZE,
    files::{PrivateArchive, PublicArchive},
    graph::{GraphEntry, GraphEntryAddress},
    pointer::{Pointer, PointerAddress},
    register::RegisterValue,
    scratchpad::Scratchpad,
    self_encryption::DataMapLevel,
};
use ant_protocol::storage::PointerTarget;
use ant_protocol::storage::ScratchpadAddress;
use futures::stream;
use futures::stream::StreamExt;
use self_encryption::ChunkInfo;
use self_encryption::DataMap;
use std::collections::HashMap;
use std::collections::VecDeque;

use super::{GetError, register::RegisterAddress};
const MAX_HEX_PRINT_LENGTH: usize = 4 * 1024;

macro_rules! println_if {
    ($cond:expr, $($arg:tt)*) => {
        if $cond {
            println!($($arg)*);
        }
    };
}

/// The result of analyzing an address
#[derive(custom_debug::Debug, Clone, Eq, PartialEq)]
pub enum Analysis {
    /// A raw chunk of data
    Chunk(Chunk),
    /// A graph entry
    GraphEntry(GraphEntry),
    /// A pointer
    Pointer(Pointer),
    /// A scratchpad
    Scratchpad(Scratchpad),
    /// A register
    Register {
        address: RegisterAddress,
        owner: PublicKey,
        underlying_graph_start: GraphEntryAddress,
        underlying_head_pointer: PointerAddress,
        current_value: RegisterValue,
    },
    /// A chunk containing a datamap
    DataMap {
        address: ChunkAddress,
        chunks: Vec<ChunkAddress>,
        points_to_a_data_map: bool,
        // Return `None` for large sized target, as it shall not be handled ALL in memory
        #[debug(skip)]
        data: Option<Bytes>,
    },
    /// A raw datamap
    RawDataMap {
        chunks: Vec<ChunkAddress>,
        points_to_a_data_map: bool,
        // Return `None` for large sized target, as it shall not be handled ALL in memory
        #[debug(skip)]
        data: Option<Bytes>,
    },
    /// A public archive
    /// (chunk containing a datamap of a public archive)
    PublicArchive {
        address: Option<ChunkAddress>,
        archive: PublicArchive,
    },
    /// A private archive
    /// (a datamap of a private archive)
    PrivateArchive(PrivateArchive),
}

impl std::fmt::Display for Analysis {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Analysis::Chunk(chunk) => {
                writeln!(f, "Chunk stored at: {}", chunk.address().to_hex())?;
            }
            Analysis::GraphEntry(graph_entry) => {
                writeln!(
                    f,
                    "GraphEntry stored at: {}",
                    graph_entry.address().to_hex()
                )?;
                writeln!(f, "{graph_entry:#?}")?;
            }
            Analysis::Pointer(pointer) => {
                writeln!(f, "Pointer stored at: {}", pointer.address().to_hex())?;
                writeln!(f, "Pointer points to: {}", pointer.target().to_hex())?;
                writeln!(f, "{pointer:#?}")?;
            }
            Analysis::Scratchpad(scratchpad) => {
                writeln!(f, "Scratchpad stored at: {}", scratchpad.address().to_hex())?;
                writeln!(f, "{scratchpad:#?}")?;
            }
            Analysis::Register {
                address,
                owner,
                underlying_graph_start,
                underlying_head_pointer,
                current_value,
            } => {
                writeln!(f, "Register stored at: {}", address.to_hex())?;
                writeln!(f, "Owner: {}", owner.to_hex())?;
                writeln!(f, "Current Value in hex: {}", hex::encode(current_value))?;
                writeln!(f, "Underlying Graph Start: {underlying_graph_start:#?}")?;
                writeln!(f, "Underlying Head Pointer: {underlying_head_pointer:#?}")?;
            }
            Analysis::DataMap {
                address,
                chunks,
                points_to_a_data_map,
                data,
            } => {
                writeln!(f, "DataMap stored at: {}", address.to_hex())?;
                writeln!(f, "DataMap containing {} Chunks", chunks.len())?;
                writeln!(f, "Content is another DataMap: {points_to_a_data_map}")?;
                writeln!(f, "{chunks:#?}")?;
                let data_hex = if let Some(data) = data {
                    data_hex(data)
                } else {
                    "None".to_string()
                };
                writeln!(f, "Decrypted Data in hex: {data_hex}")?;
            }
            Analysis::RawDataMap {
                chunks,
                points_to_a_data_map,
                data,
            } => {
                writeln!(f, "DataMap containing {} Chunks", chunks.len())?;
                writeln!(f, "Content is another DataMap: {points_to_a_data_map}")?;
                writeln!(f, "{chunks:#?}")?;
                let data_hex = if let Some(data) = data {
                    data_hex(data)
                } else {
                    "None".to_string()
                };
                writeln!(f, "Decrypted Data in hex: {data_hex}")?;
            }
            Analysis::PublicArchive { address, archive } => {
                writeln!(
                    f,
                    "PublicArchive stored at: {}",
                    address
                        .map(|a| a.to_hex())
                        .unwrap_or("Undefined".to_string())
                )?;
                writeln!(f, "PublicArchive with {} files", archive.files().len())?;
                writeln!(f, "{archive:#?}")?;
            }
            Analysis::PrivateArchive(archive) => {
                writeln!(f, "PrivateArchive with {} files", archive.files().len())?;
                writeln!(f, "{archive:#?}")?;
            }
        }
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AnalysisError {
    #[error("Input was not recognized as a valid address")]
    UnrecognizedInput,
    #[error("Failed to get data at address {0}")]
    GetError(#[from] GetError),
    #[error("Although address format is valid, failed to get data at address")]
    FailedGet,
}

impl Client {
    /// Analyze an address and return the type of the address.
    /// Can be run in verbose mode to make it talkative (will print information as it works).
    pub async fn analyze_address(
        &self,
        address: &str,
        verbose: bool,
    ) -> Result<Analysis, AnalysisError> {
        let hex_addr = address.trim_start_matches("0x");

        // data addresses
        let maybe_xorname = ChunkAddress::from_hex(address).ok();
        if let Some(chunk_addr) = maybe_xorname {
            println_if!(verbose, "Identified as a Chunk address...");
            return analyze_chunk(&chunk_addr, self, verbose).await;
        }

        // public keys
        let maybe_public_key = PublicKey::from_hex(hex_addr).ok();
        if let Some(public_key) = maybe_public_key {
            println_if!(
                verbose,
                "Identified as a bls Public Key, might be a key addressed type..."
            );
            return analyze_public_key(&public_key, self, verbose).await;
        }

        // datamaps
        if let Ok(hex_chunk) = DataMapChunk::from_hex(hex_addr) {
            println_if!(verbose, "Detected hex encoded data, might be a DataMap...");
            let maybe_data_map: Option<DataMap> = rmp_serde::from_slice(hex_chunk.0.value()).ok();
            if let Some(_data_map) = maybe_data_map {
                println_if!(verbose, "Identified as a new DataMap...");
                return analyze_datamap(None, &hex_chunk, self, verbose).await;
            }

            let maybe_data_map: Option<DataMapLevel> =
                rmp_serde::from_slice(hex_chunk.0.value()).ok();
            if let Some(_data_map) = maybe_data_map {
                println_if!(verbose, "Identified as an old DataMap...");
                return analyze_datamap_old(None, &hex_chunk, self, verbose).await;
            }
        }

        Err(AnalysisError::UnrecognizedInput)
    }

    /// Analyze an address recursively by following all discovered addresses. Returns the list of analyses found.
    /// Can be run in verbose mode to make it talkative (will print information as it works).
    pub async fn analyze_address_recursively(
        &self,
        address: &str,
        verbose: bool,
    ) -> HashMap<String, Result<Analysis, AnalysisError>> {
        let mut results = HashMap::new();
        let mut to_process: VecDeque<String> = VecDeque::new();
        to_process.push_back(address.to_string());

        while !to_process.is_empty() {
            // Take addresses to process in this batch
            let batch: Vec<String> = to_process
                .drain(..)
                .filter(|addr| !results.contains_key(addr))
                .collect();

            if batch.is_empty() {
                break;
            }
            println_if!(verbose, "Analyzing batch of {} addresses", batch.len());
            info!("Analyzing batch of {} addresses", batch.len());

            // Create futures for the batch
            let analyze_tasks = batch.into_iter().map(|addr| {
                let client = self.clone();
                async move {
                    println!("Analyzing address: {addr}");
                    info!("Analyzing address: {addr}");
                    let analysis = client.analyze_address(&addr, false).await;
                    info!("Analysis completed for address: {addr}, result: {analysis:?}");

                    let referenced_addrs = if let Ok(ref analysis_result) = analysis {
                        let addrs = extract_addresses(analysis_result);
                        if !addrs.is_empty() {
                            println_if!(
                                verbose,
                                "Found {} referenced addresses from {addr}",
                                addrs.len(),
                            );
                            info!("Found {} referenced addresses from {addr}", addrs.len());
                        }
                        addrs
                    } else {
                        Vec::new()
                    };

                    (addr, analysis, referenced_addrs)
                }
            });
            let batch_results: Vec<_> = stream::iter(analyze_tasks)
                .buffered(*CHUNK_DOWNLOAD_BATCH_SIZE)
                .collect()
                .await;

            // Process results
            for (addr, analysis, referenced_addrs) in batch_results {
                results.insert(addr, analysis);

                // Add new addresses to process
                for ref_addr in referenced_addrs {
                    if !results.contains_key(&ref_addr) && !to_process.contains(&ref_addr) {
                        to_process.push_back(ref_addr);
                    }
                }
            }
        }
        println!(
            "Finished recursive analysis. Total addresses analyzed: {}",
            results.len()
        );

        results
    }

    /// Analyze an address and return the address type (Chunk, Pointer, GraphEntry, Scratchpad)
    pub async fn analyze_address_type(
        &self,
        address: &str,
        verbose: bool,
    ) -> Result<PointerTarget, AnalysisError> {
        let hex_addr = address.trim_start_matches("0x");

        // data addresses
        let maybe_xorname = ChunkAddress::from_hex(address).ok();
        if let Some(chunk_addr) = maybe_xorname {
            println_if!(verbose, "Identified as a Chunk address...");
            return Ok(PointerTarget::ChunkAddress(chunk_addr));
        }

        // public keys
        let maybe_public_key = PublicKey::from_hex(hex_addr).ok();
        if let Some(public_key) = maybe_public_key {
            println_if!(
                verbose,
                "Identified as a bls Public Key, might be a key addressed type..."
            );
            return analyze_public_key_type(public_key, self, verbose).await;
        }

        Err(AnalysisError::UnrecognizedInput)
    }
}

async fn analyze_public_key_type(
    public_key: PublicKey,
    client: &Client,
    verbose: bool,
) -> Result<PointerTarget, AnalysisError> {
    let graph_entry_address = GraphEntryAddress::new(public_key);
    let pointer_address = PointerAddress::new(public_key);
    let scratchpad_address = ScratchpadAddress::new(public_key);

    let graph_entry_res = client.graph_entry_get(&graph_entry_address);
    let pointer_res = client.pointer_get(&pointer_address);
    let scratchpad_res = client.scratchpad_get(&scratchpad_address);

    let (maybe_graph_entry, maybe_pointer, maybe_scratchpad) =
        tokio::join!(graph_entry_res, pointer_res, scratchpad_res);

    match (maybe_graph_entry, maybe_pointer, maybe_scratchpad) {
        (Ok(_graph_entry), _, _) => {
            println_if!(verbose, "Identified GraphEntry...");
            Ok(PointerTarget::GraphEntryAddress(graph_entry_address))
        }
        (_, Ok(_pointer), _) => {
            println_if!(verbose, "Identified Pointer...");
            Ok(PointerTarget::PointerAddress(pointer_address))
        }
        (_, _, Ok(_scratchpad)) => {
            println_if!(verbose, "Identified Scratchpad...");
            Ok(PointerTarget::ScratchpadAddress(scratchpad_address))
        }
        (Err(e1), Err(e2), Err(e3)) => {
            println_if!(verbose, "Failed to get graph entry: {e1}");
            println_if!(verbose, "Failed to get pointer: {e2}");
            println_if!(verbose, "Failed to get scratchpad: {e3}");
            Err(AnalysisError::FailedGet)
        }
    }
}

async fn analyze_chunk(
    chunk_addr: &ChunkAddress,
    client: &Client,
    verbose: bool,
) -> Result<Analysis, AnalysisError> {
    println_if!(
        verbose,
        "Getting chunk at address: {} ...",
        chunk_addr.to_hex()
    );
    let chunk = client.chunk_get(chunk_addr).await?;
    println_if!(verbose, "Got chunk of {} bytes...", chunk.value().len());

    // check if it's a datamap
    if let Ok(_data_map) = rmp_serde::from_slice::<DataMap>(chunk.value()) {
        println_if!(verbose, "Identified chunk content as a DataMap...");
        return analyze_datamap(Some(*chunk_addr), &chunk.into(), client, verbose).await;
    }

    // check if it's an old datamap
    if let Ok(_data_map) = rmp_serde::from_slice::<DataMapLevel>(chunk.value()) {
        println_if!(verbose, "Identified chunk content as an old DataMap...");
        return analyze_datamap_old(Some(*chunk_addr), &chunk.into(), client, verbose).await;
    }

    Ok(Analysis::Chunk(chunk))
}

async fn analyze_datamap(
    stored_at: Option<ChunkAddress>,
    datamap: &DataMapChunk,
    client: &Client,
    verbose: bool,
) -> Result<Analysis, AnalysisError> {
    let map: DataMap =
        rmp_serde::from_slice(datamap.0.value()).map_err(|_| AnalysisError::UnrecognizedInput)?;
    let points_to_a_data_map = map.child.is_some();

    println_if!(verbose, "Fetching data from the Network...");
    let data = match client.data_get(datamap).await {
        Ok(data) => data,
        Err(GetError::TooLargeForMemory) => {
            println_if!(
                verbose,
                "Datamap points to a large sized file, not suitable for in-memory fetch."
            );
            let analysis = match stored_at {
                Some(addr) => Analysis::DataMap {
                    address: addr,
                    chunks: chunk_list_from_datamap(map),
                    data: None,
                    points_to_a_data_map,
                },
                None => Analysis::RawDataMap {
                    chunks: chunk_list_from_datamap(map),
                    data: None,
                    points_to_a_data_map,
                },
            };
            return Ok(analysis);
        }
        Err(e) => return Err(AnalysisError::GetError(e)),
    };
    println_if!(verbose, "Data fetched from the Network...");

    if let Ok(private_archive) = PrivateArchive::from_bytes(data.clone()) {
        // public archives and private archives can be confused into each other
        // to identify them we check if all the addresses are in fact xornames
        // cf test_archives_serialize_deserialize for more details
        let xorname_hex_len = xor_name::XOR_NAME_LEN * 2;
        let all_addrs_are_xornames = private_archive
            .map()
            .iter()
            .all(|(_, (data_addr, _))| data_addr.to_hex().len() == xorname_hex_len);
        if all_addrs_are_xornames {
            println_if!(
                verbose,
                "All addresses are xornames, so it's a public archive"
            );
            if let Ok(public_archive) = PublicArchive::from_bytes(data.clone()) {
                println_if!(
                    verbose,
                    "Identified the data pointed to by the DataMap as a PublicArchive..."
                );
                return Ok(Analysis::PublicArchive {
                    address: stored_at,
                    archive: public_archive,
                });
            }
        }

        println_if!(
            verbose,
            "Identified the data pointed to by the DataMap as a PrivateArchive..."
        );
        return Ok(Analysis::PrivateArchive(private_archive));
    }

    if let Ok(public_archive) = PublicArchive::from_bytes(data.clone()) {
        println_if!(
            verbose,
            "Identified the data pointed to by the DataMap as a PublicArchive..."
        );
        return Ok(Analysis::PublicArchive {
            address: stored_at,
            archive: public_archive,
        });
    }

    let analysis = match stored_at {
        Some(addr) => Analysis::DataMap {
            address: addr,
            chunks: chunk_list_from_datamap(map),
            data: Some(data),
            points_to_a_data_map,
        },
        None => Analysis::RawDataMap {
            chunks: chunk_list_from_datamap(map),
            data: Some(data),
            points_to_a_data_map,
        },
    };

    Ok(analysis)
}

async fn analyze_datamap_old(
    stored_at: Option<ChunkAddress>,
    datamap: &DataMapChunk,
    client: &Client,
    verbose: bool,
) -> Result<Analysis, AnalysisError> {
    let data_map_level: DataMapLevel =
        rmp_serde::from_slice(datamap.0.value()).map_err(|_| AnalysisError::UnrecognizedInput)?;
    let (map, points_to_a_data_map) = match data_map_level {
        DataMapLevel::Additional(map) => {
            println_if!(
                verbose,
                "Identified a DataMap whose contents is another DataMap, the content might be pretty big..."
            );
            (map, true)
        }
        DataMapLevel::First(map) => {
            println_if!(
                verbose,
                "Identified a DataMap which directly contains data..."
            );
            (map, false)
        }
    };

    // Convert old format of DataMap into new
    let chunk_identifiers: Vec<ChunkInfo> = map
        .infos()
        .iter()
        .map(|ck_info| ChunkInfo {
            index: ck_info.index,
            dst_hash: ck_info.dst_hash,
            src_hash: ck_info.src_hash,
            src_size: ck_info.src_size,
        })
        .collect();
    let data_map = DataMap {
        chunk_identifiers,
        child: None,
    };

    println_if!(verbose, "Fetching data from the Network...");
    let data = match client.data_get(datamap).await {
        Ok(data) => data,
        Err(GetError::TooLargeForMemory) => {
            println_if!(
                verbose,
                "Datamap points to a large sized file, not suitable for in-memory fetch."
            );
            let analysis = match stored_at {
                Some(addr) => Analysis::DataMap {
                    address: addr,
                    chunks: chunk_list_from_datamap(data_map),
                    data: None,
                    points_to_a_data_map,
                },
                None => Analysis::RawDataMap {
                    chunks: chunk_list_from_datamap(data_map),
                    data: None,
                    points_to_a_data_map,
                },
            };
            return Ok(analysis);
        }
        Err(e) => return Err(AnalysisError::GetError(e)),
    };
    println_if!(verbose, "Data fetched from the Network...");

    if let Ok(private_archive) = PrivateArchive::from_bytes(data.clone()) {
        // public archives and private archives can be confused into each other
        // to identify them we check if all the addresses are in fact xornames
        // cf test_archives_serialize_deserialize for more details
        let xorname_hex_len = xor_name::XOR_NAME_LEN * 2;
        let all_addrs_are_xornames = private_archive
            .map()
            .iter()
            .all(|(_, (data_addr, _))| data_addr.to_hex().len() == xorname_hex_len);
        if all_addrs_are_xornames {
            println_if!(
                verbose,
                "All addresses are xornames, so it's a public archive"
            );
            if let Ok(public_archive) = PublicArchive::from_bytes(data.clone()) {
                println_if!(
                    verbose,
                    "Identified the data pointed to by the DataMap as a PublicArchive..."
                );
                return Ok(Analysis::PublicArchive {
                    address: stored_at,
                    archive: public_archive,
                });
            }
        }

        println_if!(
            verbose,
            "Identified the data pointed to by the DataMap as a PrivateArchive..."
        );
        return Ok(Analysis::PrivateArchive(private_archive));
    }

    if let Ok(public_archive) = PublicArchive::from_bytes(data.clone()) {
        println_if!(
            verbose,
            "Identified the data pointed to by the DataMap as a PublicArchive..."
        );
        return Ok(Analysis::PublicArchive {
            address: stored_at,
            archive: public_archive,
        });
    }

    let analysis = match stored_at {
        Some(addr) => Analysis::DataMap {
            address: addr,
            chunks: chunk_list_from_datamap(data_map),
            data: Some(data),
            points_to_a_data_map,
        },
        None => Analysis::RawDataMap {
            chunks: chunk_list_from_datamap(data_map),
            data: Some(data),
            points_to_a_data_map,
        },
    };

    Ok(analysis)
}

async fn analyze_public_key(
    public_key: &PublicKey,
    client: &Client,
    verbose: bool,
) -> Result<Analysis, AnalysisError> {
    let graph_entry_address = GraphEntryAddress::new(*public_key);
    let pointer_address = PointerAddress::new(*public_key);
    let register_address = RegisterAddress::new(*public_key);
    let scratchpad_address = ScratchpadAddress::new(*public_key);

    let graph_entry_res = client.graph_entry_get(&graph_entry_address);
    let pointer_res = client.pointer_get(&pointer_address);
    let register_res = client.register_get(&register_address);
    let scratchpad_res = client.scratchpad_get(&scratchpad_address);

    let (maybe_graph_entry, maybe_pointer, maybe_register, maybe_scratchpad) =
        tokio::join!(graph_entry_res, pointer_res, register_res, scratchpad_res);

    match (
        maybe_graph_entry,
        maybe_pointer,
        maybe_register,
        maybe_scratchpad,
    ) {
        (graph_entry, _, Ok(value), _) => {
            println_if!(
                verbose,
                "Identified GraphEntry, which fits the format of a Register..."
            );
            println_if!(verbose, "GraphEntry: {:#?}", graph_entry);
            Ok(Analysis::Register {
                address: register_address,
                owner: *public_key,
                underlying_graph_start: register_address.to_underlying_graph_root(),
                underlying_head_pointer: register_address.to_underlying_head_pointer(),
                current_value: value,
            })
        }
        (Ok(graph_entry), _, _, _) => {
            println_if!(verbose, "Identified GraphEntry...");
            Ok(Analysis::GraphEntry(graph_entry))
        }
        (_, Ok(pointer), _, _) => {
            println_if!(verbose, "Identified Pointer...");
            Ok(Analysis::Pointer(pointer))
        }
        (_, _, _, Ok(scratchpad)) => {
            println_if!(verbose, "Identified Scratchpad...");
            Ok(Analysis::Scratchpad(scratchpad))
        }
        (Err(e1), Err(e2), Err(e3), Err(e4)) => {
            println_if!(verbose, "Failed to get graph entry: {e1}");
            println_if!(verbose, "Failed to get pointer: {e2}");
            println_if!(verbose, "Failed to get register: {e3}");
            println_if!(verbose, "Failed to get scratchpad: {e4}");
            Err(AnalysisError::FailedGet)
        }
    }
}

fn chunk_list_from_datamap(map: DataMap) -> Vec<ChunkAddress> {
    let mut chunks = Vec::new();
    for info in map.infos() {
        chunks.push(ChunkAddress::new(info.dst_hash));
    }
    chunks
}

fn data_hex(data: &Bytes) -> String {
    if data.len() <= MAX_HEX_PRINT_LENGTH {
        hex::encode(data)
    } else {
        format!("[{} bytes of data]", data.len())
    }
}

fn extract_addresses(analysis: &Analysis) -> Vec<String> {
    let mut addresses = Vec::new();

    match analysis {
        Analysis::GraphEntry(graph_entry) => {
            // Extract descendant public keys from graph entry
            info!(
                "Extracting addresses from GraphEntry descendants... len: {}",
                graph_entry.descendants.len()
            );
            for (public_key, _content) in graph_entry.descendants.iter() {
                addresses.push(public_key.to_hex());
            }
        }
        Analysis::Pointer(pointer) => {
            info!("Extracting target address from Pointer... len: 1");
            addresses.push(pointer.target().to_hex());
        }
        Analysis::DataMap { chunks, .. } | Analysis::RawDataMap { chunks, .. } => {
            // Extract all chunk addresses
            info!(
                "Extracting chunk addresses from DataMap... len: {}",
                chunks.len()
            );
            for chunk_addr in chunks {
                addresses.push(chunk_addr.to_hex());
            }
        }
        Analysis::PublicArchive { address, .. } => {
            if let Some(addr) = address {
                let addr = addr.to_hex();
                info!("Extracting 1 address from PublicArchive: {addr}");
                addresses.push(addr);
            }
        }
        Analysis::Chunk(_)
        | Analysis::Scratchpad(_)
        | Analysis::Register { .. }
        | Analysis::PrivateArchive(_) => {
            // These types don't point to other data
        }
    }

    addresses
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::Bytes;
    use crate::chunk::{Chunk, ChunkAddress, DataMapChunk};
    use crate::data::DataAddress;
    use crate::files::{Metadata, PrivateArchive, PublicArchive};
    use crate::self_encryption::DataMapLevel;
    use crate::{PublicKey, SecretKey};
    use eyre::Result;
    use self_encryption::{ChunkInfo, DataMap};
    use self_encryption_old::DataMap as OldDataMap;
    use serial_test::serial;
    use xor_name::XorName;

    // this test confirms that a xorname and a public key are different and can't be confused for each other
    #[tokio::test]
    #[serial]
    async fn test_xorname_pubkey_different() -> Result<()> {
        let xorname = xor_name::XorName::random(&mut rand::thread_rng());
        let pubkey = SecretKey::random().public_key();
        let xorname_hex = ChunkAddress::new(xorname).to_hex();
        let pubkey_hex = pubkey.to_hex();

        let maybe_xorname = ChunkAddress::from_hex(&pubkey_hex);
        assert!(maybe_xorname.is_err());
        let maybe_public_key = PublicKey::from_hex(&xorname_hex);
        assert!(maybe_public_key.is_err());
        Ok(())
    }

    // this test confirms that both public and private archives can serialize and deserialize into each other
    // this is an unfortunate side effect of the fact Bytes and XorName can be serialized and deserialized into each other
    // this is due to the fact that XorName deserialization accepts any data longer or equal to 32 bytes
    #[tokio::test]
    #[serial]
    async fn test_archives_serialize_deserialize() -> Result<()> {
        // create a public and private archive
        let mut archive_public = PublicArchive::new();
        let mut archive = PrivateArchive::new();

        // add a files to the archive
        let data_addr = DataAddress::new(xor_name::XorName::random(&mut rand::thread_rng()));
        let datamap = DataMapChunk::from_hex(
            "123412341234123412341234123412341243123412341243123412341234123412341234",
        )
        .unwrap();
        archive_public.add_file(PathBuf::from("filename"), data_addr, Metadata::default());
        archive.add_file(PathBuf::from("filename"), datamap, Metadata::default());

        // they can be serialized and deserialized into each other
        let serialized = rmp_serde::to_vec_named(&archive_public).unwrap();
        let deserialized_as_pub: PublicArchive = rmp_serde::from_slice(&serialized).unwrap();
        let deserialized_as_private: PrivateArchive = rmp_serde::from_slice(&serialized).unwrap();
        assert_eq!(archive_public, deserialized_as_pub);
        println!("Deserialized as private: {deserialized_as_private:#?}");
        println!("Deserialized as public: {deserialized_as_pub:#?}");

        // the other way around is also possible
        let serialized = rmp_serde::to_vec_named(&archive).unwrap();
        let deserialized_as_pub: PublicArchive = rmp_serde::from_slice(&serialized).unwrap();
        let deserialized_as_private: PrivateArchive = rmp_serde::from_slice(&serialized).unwrap();
        assert_eq!(archive, deserialized_as_private);
        println!("Deserialized as private: {deserialized_as_private:#?}");
        println!("Deserialized as public: {deserialized_as_pub:#?}");

        // bytes and XorName can be serialized and deserialized into each other
        let mut rng = rand::thread_rng();
        let xorname = xor_name::XorName::random(&mut rng);
        let bytes = Bytes::from("whatever data longer than 32 bytes would be okay");
        let serialized = rmp_serde::to_vec_named(&xorname).unwrap();
        let deserialized_as_xorname: xor_name::XorName =
            rmp_serde::from_slice(&serialized).unwrap();
        let _deserialized_as_bytes: Bytes = rmp_serde::from_slice(&serialized).unwrap();
        assert_eq!(xorname, deserialized_as_xorname);

        // and vice versa
        let serialized = rmp_serde::to_vec_named(&bytes).unwrap();
        let deserialized_as_bytes: Bytes = rmp_serde::from_slice(&serialized).unwrap();
        let _deserialized_as_xorname: xor_name::XorName =
            rmp_serde::from_slice(&serialized).unwrap();
        assert_eq!(bytes, deserialized_as_bytes);

        Ok(())
    }

    #[tokio::test]
    async fn test_analyze_new_datamap_format() {
        // Create a mock new format DataMap
        let chunk_identifiers = vec![
            ChunkInfo {
                index: 0,
                dst_hash: XorName::random(&mut rand::thread_rng()),
                src_hash: XorName::random(&mut rand::thread_rng()),
                src_size: 1024,
            },
            ChunkInfo {
                index: 1,
                dst_hash: XorName::random(&mut rand::thread_rng()),
                src_hash: XorName::random(&mut rand::thread_rng()),
                src_size: 1024,
            },
        ];

        let data_map = DataMap {
            chunk_identifiers,
            child: None,
        };

        // Serialize the DataMap
        let serialized = rmp_serde::to_vec_named(&data_map).unwrap();
        let chunk = Chunk::new(Bytes::from(serialized));
        let _chunk_addr = *chunk.address();

        // Test that we can identify it as a new DataMap
        let datamap_chunk = DataMapChunk(chunk.clone());
        let deserialized: Result<DataMap, _> = rmp_serde::from_slice(datamap_chunk.0.value());
        assert!(
            deserialized.is_ok(),
            "Should deserialize as new DataMap format"
        );

        // Tests that we should not be able to deserialize it as an old DataMap
        let old_format_result: Result<OldDataMap, _> =
            rmp_serde::from_slice(datamap_chunk.0.value());
        assert!(
            old_format_result.is_err(),
            "Should NOT deserialize as old DataMap format"
        );
    }

    #[tokio::test]
    async fn test_analyze_old_datamap_format() {
        // Create a mock old format DataMap (DataMapLevel::First)
        let old_data_map = OldDataMap::new(vec![]);

        let data_map_level = DataMapLevel::First(old_data_map);

        // Serialize the old format DataMapLevel
        let serialized = rmp_serde::to_vec_named(&data_map_level).unwrap();
        let chunk = Chunk::new(Bytes::from(serialized));
        let _chunk_addr = *chunk.address();

        // Test that we can identify it as an old DataMap
        let datamap_chunk = DataMapChunk(chunk.clone());

        // It should NOT deserialize as new format
        let new_format_result: Result<DataMap, _> = rmp_serde::from_slice(datamap_chunk.0.value());
        assert!(
            new_format_result.is_err(),
            "Should NOT deserialize as new DataMap format"
        );

        // It SHOULD deserialize as old format
        let old_format_result: Result<DataMapLevel, _> =
            rmp_serde::from_slice(datamap_chunk.0.value());
        assert!(
            old_format_result.is_ok(),
            "Should deserialize as old DataMapLevel format"
        );
    }

    #[tokio::test]
    async fn test_analyze_old_datamap_additional_level() {
        // Create a mock old format DataMap (DataMapLevel::Additional)
        let old_data_map = OldDataMap::new(vec![]);

        let data_map_level = DataMapLevel::Additional(old_data_map);

        // Serialize the old format DataMapLevel
        let serialized = rmp_serde::to_vec_named(&data_map_level).unwrap();
        let chunk = Chunk::new(Bytes::from(serialized));

        // Test that we can identify it as an old DataMap with additional level
        let datamap_chunk = DataMapChunk(chunk.clone());

        // It should NOT deserialize as new format
        let new_format_result: Result<DataMap, _> = rmp_serde::from_slice(datamap_chunk.0.value());
        assert!(
            new_format_result.is_err(),
            "Should NOT deserialize as new DataMap format"
        );

        // It SHOULD deserialize as old format
        let old_format_result: Result<DataMapLevel, _> =
            rmp_serde::from_slice(datamap_chunk.0.value());
        assert!(
            old_format_result.is_ok(),
            "Should deserialize as old DataMapLevel format"
        );

        // Verify it's an Additional level
        match old_format_result.unwrap() {
            DataMapLevel::Additional(_) => {}
            _ => panic!("Expected DataMapLevel::Additional"),
        }
    }
}
