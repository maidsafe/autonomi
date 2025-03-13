// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use ant_protocol::storage::ScratchpadAddress;
use self_encryption::DataMap;

use crate::{
    chunk::{Chunk, ChunkAddress, DataMapChunk},
    files::{PrivateArchive, PublicArchive},
    graph::{GraphEntry, GraphEntryAddress},
    pointer::{Pointer, PointerAddress},
    register::RegisterValue,
    scratchpad::Scratchpad,
    self_encryption::DataMapLevel,
    Bytes, Client, PublicKey,
};

use super::{register::RegisterAddress, GetError};
const MAX_HEX_PRINT_LENGTH: usize = 4 * 1024;

/// The result of analyzing an address
#[derive(Debug, Clone, Eq, PartialEq)]
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
    /// A chunk containing a data map
    DataMap {
        address: ChunkAddress,
        chunks: Vec<ChunkAddress>,
        points_to_a_data_map: bool,
        data: Bytes,
    },
    /// A raw data map
    RawDataMap {
        chunks: Vec<ChunkAddress>,
        points_to_a_data_map: bool,
        data: Bytes,
    },
    /// A public archive
    /// (chunk containing a data map of a public archive)
    PublicArchive {
        address: Option<ChunkAddress>,
        archive: PublicArchive,
    },
    /// A private archive
    /// (a data map of a private archive)
    PrivateArchive(PrivateArchive),
}

impl std::fmt::Display for Analysis {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Analysis::Chunk(chunk) => {
                writeln!(f, "Chunk stored at: {}", chunk.address().to_hex())?;
                writeln!(f, "Chunk content in hex: {}", hex::encode(chunk.value()))?;
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
                writeln!(f, "Decrypted Data in hex: {}", data_hex(data))?;
            }
            Analysis::RawDataMap {
                chunks,
                points_to_a_data_map,
                data,
            } => {
                writeln!(f, "DataMap containing {} Chunks", chunks.len())?;
                writeln!(f, "Content is another DataMap: {points_to_a_data_map}")?;
                writeln!(f, "{chunks:#?}")?;
                writeln!(f, "Decrypted Data in hex: {}", data_hex(data))?;
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
        macro_rules! println_if_verbose {
            ($($arg:tt)*) => {
                if verbose {
                    println!($($arg)*);
                }
            };
        }
        let hex_addr = address.trim_start_matches("0x");

        // data addresses
        let maybe_xorname = ChunkAddress::from_hex(address).ok();
        if let Some(chunk_addr) = maybe_xorname {
            println_if_verbose!("Identified as a Chunk address...");
            return analyze_chunk(&chunk_addr, self, verbose).await;
        }

        // public keys
        let maybe_public_key = PublicKey::from_hex(hex_addr).ok();
        if let Some(public_key) = maybe_public_key {
            println_if_verbose!("Identified as a bls Public Key, might be a key addressed type...");
            return analyze_public_key(&public_key, self, verbose).await;
        }

        // datamaps
        if let Ok(hex_chunk) = DataMapChunk::from_hex(hex_addr) {
            println_if_verbose!("Detected hex encoded data, might be a DataMap...");
            let maybe_data_map: Option<DataMapLevel> =
                rmp_serde::from_slice(hex_chunk.0.value()).ok();
            if let Some(_data_map) = maybe_data_map {
                println_if_verbose!("Identified as a DataMap...");
                return analyze_datamap(None, &hex_chunk, self, verbose).await;
            }
        }

        Err(AnalysisError::UnrecognizedInput)
    }
}

async fn analyze_chunk(
    chunk_addr: &ChunkAddress,
    client: &Client,
    verbose: bool,
) -> Result<Analysis, AnalysisError> {
    macro_rules! println_if_verbose {
        ($($arg:tt)*) => {
            if verbose {
                println!($($arg)*);
            }
        };
    }

    println_if_verbose!("Getting chunk at address: {} ...", chunk_addr.to_hex());
    let chunk = client.chunk_get(chunk_addr).await?;
    println_if_verbose!("Got chunk of {} bytes...", chunk.value().len());

    // check if it's a data map
    if let Ok(_data_map) = rmp_serde::from_slice::<DataMapLevel>(chunk.value()) {
        println_if_verbose!("Identified chunk content as a DataMap...");
        return analyze_datamap(Some(*chunk_addr), &chunk.into(), client, verbose).await;
    }

    Ok(Analysis::Chunk(chunk))
}

async fn analyze_datamap(
    stored_at: Option<ChunkAddress>,
    datamap: &DataMapChunk,
    client: &Client,
    verbose: bool,
) -> Result<Analysis, AnalysisError> {
    macro_rules! println_if_verbose {
        ($($arg:tt)*) => {
            if verbose {
                println!($($arg)*);
            }
        };
    }

    let data_map_level: DataMapLevel =
        rmp_serde::from_slice(datamap.0.value()).map_err(|_| AnalysisError::UnrecognizedInput)?;
    let (map, points_to_a_data_map) = match data_map_level {
        DataMapLevel::Additional(map) => {
            println_if_verbose!("Identified a DataMap whose contents is another DataMap, the content might be pretty big...");
            (map, true)
        }
        DataMapLevel::First(map) => {
            println_if_verbose!("Identified a DataMap which directly contains data...");
            (map, false)
        }
    };

    println_if_verbose!("Fetching data from the Network...");
    let data = client.data_get(datamap).await?;
    println_if_verbose!("Data fetched from the Network...");

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
            println_if_verbose!("All addresses are xornames, so it's a public archive");
            if let Ok(public_archive) = PublicArchive::from_bytes(data.clone()) {
                println_if_verbose!(
                    "Identified the data pointed to by the DataMap as a PublicArchive..."
                );
                return Ok(Analysis::PublicArchive {
                    address: stored_at,
                    archive: public_archive,
                });
            }
        }

        println_if_verbose!("Identified the data pointed to by the DataMap as a PrivateArchive...");
        return Ok(Analysis::PrivateArchive(private_archive));
    }

    if let Ok(public_archive) = PublicArchive::from_bytes(data.clone()) {
        println_if_verbose!("Identified the data pointed to by the DataMap as a PublicArchive...");
        return Ok(Analysis::PublicArchive {
            address: stored_at,
            archive: public_archive,
        });
    }

    let analysis = match stored_at {
        Some(addr) => Analysis::DataMap {
            address: addr,
            chunks: chunk_list_from_datamap(map),
            data,
            points_to_a_data_map,
        },
        None => Analysis::RawDataMap {
            chunks: chunk_list_from_datamap(map),
            data,
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
    macro_rules! println_if_verbose {
        ($($arg:tt)*) => {
            if verbose {
                println!($($arg)*);
            }
        };
    }

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
            println_if_verbose!("Identified GraphEntry, which fits the format of a Register...");
            println_if_verbose!("GraphEntry: {:#?}", graph_entry);
            Ok(Analysis::Register {
                address: register_address,
                owner: *public_key,
                underlying_graph_start: register_address.to_underlying_graph_root(),
                underlying_head_pointer: register_address.to_underlying_head_pointer(),
                current_value: value,
            })
        }
        (Ok(graph_entry), _, _, _) => {
            println_if_verbose!("Identified GraphEntry...");
            Ok(Analysis::GraphEntry(graph_entry))
        }
        (_, Ok(pointer), _, _) => {
            println_if_verbose!("Identified Pointer...");
            Ok(Analysis::Pointer(pointer))
        }
        (_, _, _, Ok(scratchpad)) => {
            println_if_verbose!("Identified Scratchpad...");
            Ok(Analysis::Scratchpad(scratchpad))
        }
        (Err(e1), Err(e2), Err(e3), Err(e4)) => {
            println_if_verbose!("Failed to get graph entry: {e1}");
            println_if_verbose!("Failed to get pointer: {e2}");
            println_if_verbose!("Failed to get register: {e3}");
            println_if_verbose!("Failed to get scratchpad: {e4}");
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::chunk::{ChunkAddress, DataMapChunk};
    use crate::data::DataAddress;
    use crate::files::{Metadata, PrivateArchive, PublicArchive};
    use crate::Bytes;
    use crate::{PublicKey, SecretKey};
    use eyre::Result;
    use serial_test::serial;

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
}
