// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! Generic record retrieval implementation for the Autonomi core client
//!
//! This module provides a unified interface for retrieving different types of records
//! from the network, with appropriate conflict resolution for each data type.

use super::{Client, DataContent, Error, GetError};
use crate::client::utils::determine_data_type_from_address;
use crate::networking::{NetworkError, Strategy};

use ant_protocol::storage::{Chunk, DataTypes, RecordHeader, RecordKind, try_deserialize_record};
use ant_protocol::{NetworkAddress, PrettyPrintRecordKey};
use libp2p::{PeerId, kad::Record};
use std::collections::HashMap;
use tracing::{debug, error, warn};

impl Client {
    /// Retrieve a record from the network using its address.
    /// Note: as `NetworkAddress` indicates the data type, the handling workflow can use that to deploy correspondent `conflict handling` scheme during the fetch.
    pub async fn record_get(&self, address: &NetworkAddress) -> Result<DataContent, Error> {
        // Determine the data type from the address
        let data_type = determine_data_type_from_address(address)?;
        let strategy = self.get_strategy(data_type);

        debug!("Fetching {:?} from network at: {:?}", data_type, address);

        match data_type {
            DataTypes::Chunk => {
                let record = self
                    .network
                    .get_record_with_retries(address.clone(), strategy)
                    .await
                    .map_err(|err| Error::GetError(GetError::Network(err)))?
                    .ok_or(Error::GetError(GetError::RecordNotFound))?;

                let chunk = deserialize_chunk_from_record(&record)?;
                Ok(DataContent::Chunk(chunk))
            }
            DataTypes::GraphEntry => {
                let record = self
                    .network
                    .get_record_with_retries(address.clone(), strategy)
                    .await
                    .map_err(|err| Error::GetError(GetError::Network(err)))?
                    .ok_or(Error::GetError(GetError::RecordNotFound))?;

                let graph_entries = deserialize_graph_entries_from_record(&record)?;
                match &graph_entries[..] {
                    [entry] => Ok(DataContent::GraphEntry(entry.clone())),
                    [] => {
                        error!("Got no valid graphentry for {address:?}");
                        Err(Error::GetError(GetError::Configuration(format!(
                            "Corrupt graphentry at {address:?}"
                        ))))
                    }
                    multiple => {
                        // Handle graph entry fork - return the first one for now
                        warn!(
                            "Graph entry fork detected at {address:?}, returning all {} entries",
                            multiple.len()
                        );
                        Ok(DataContent::GraphEntrySplit(multiple.to_vec()))
                    }
                }
            }
            DataTypes::Pointer => {
                let pointer = match self
                    .network
                    .get_record_with_retries(address.clone(), strategy)
                    .await
                {
                    Ok(Some(record)) => deserialize_pointer_from_record(&record)?,
                    Ok(None) => return Err(Error::GetError(GetError::RecordNotFound)),
                    Err(NetworkError::SplitRecord(result_map)) => {
                        warn!("Pointer at {address:?} is split, trying resolution");
                        select_highest_pointer_version(result_map, address.clone())?
                    }
                    Err(err) => {
                        error!("Error fetching pointer: {err:?}");
                        return Err(Error::GetError(GetError::Network(err)));
                    }
                };

                verify_pointer(&pointer)?;
                Ok(DataContent::Pointer(pointer))
            }
            DataTypes::Scratchpad => {
                match self
                    .network
                    .get_record_with_retries(address.clone(), strategy)
                    .await
                {
                    Ok(Some(record)) => {
                        let scratchpad = deserialize_scratchpad_from_record(&record)?;
                        verify_scratchpad(&scratchpad)?;
                        Ok(DataContent::Scratchpad(scratchpad))
                    }
                    Ok(None) => Err(Error::GetError(GetError::RecordNotFound)),
                    Err(NetworkError::SplitRecord(result_map)) => {
                        debug!("Got multiple scratchpads for {address:?}");
                        Ok(resolve_scratchpad_split(result_map, address)?)
                    }
                    Err(e) => {
                        warn!("Failed to fetch scratchpad {address:?} from network: {e}");
                        Err(Error::GetError(GetError::Network(e)))
                    }
                }
            }
        }
    }

    pub(crate) fn get_strategy(&self, data_type: DataTypes) -> &Strategy {
        match data_type {
            DataTypes::Chunk => &self.config.chunks,
            DataTypes::GraphEntry => &self.config.graph_entry,
            DataTypes::Pointer => &self.config.pointer,
            DataTypes::Scratchpad => &self.config.scratchpad,
        }
    }
}

// Helper methods for record deserialization and conflict resolution

pub(crate) fn deserialize_chunk_from_record(record: &Record) -> Result<Chunk, Error> {
    let header =
        RecordHeader::from_record(record).map_err(|e| Error::GetError(GetError::Protocol(e)))?;

    if matches!(
        header.kind,
        RecordKind::DataOnly(DataTypes::Chunk) | RecordKind::DataWithPayment(DataTypes::Chunk)
    ) {
        let chunk: Chunk =
            try_deserialize_record(record).map_err(|e| Error::GetError(GetError::Protocol(e)))?;
        Ok(chunk)
    } else {
        error!(
            "Record kind mismatch: expected Chunk, got {:?}",
            header.kind
        );
        Err(Error::GetError(GetError::RecordKindMismatch(
            RecordKind::DataOnly(DataTypes::Chunk),
        )))
    }
}

fn deserialize_graph_entries_from_record(
    record: &Record,
) -> Result<Vec<ant_protocol::storage::GraphEntry>, Error> {
    let header = RecordHeader::from_record(record).map_err(|_| {
        Error::GetError(GetError::Configuration(format!(
            "Failed to deserialize record header {:?}",
            PrettyPrintRecordKey::from(&record.key)
        )))
    })?;

    if let RecordKind::DataOnly(DataTypes::GraphEntry) = header.kind {
        let entries = try_deserialize_record::<Vec<ant_protocol::storage::GraphEntry>>(record)
            .map_err(|_| {
                Error::GetError(GetError::Configuration(format!(
                    "Failed to deserialize record value {:?}",
                    PrettyPrintRecordKey::from(&record.key)
                )))
            })?;
        Ok(entries)
    } else {
        warn!(
            "RecordKind mismatch while trying to retrieve graph_entry from record {:?}",
            PrettyPrintRecordKey::from(&record.key)
        );
        Err(Error::GetError(GetError::Configuration(format!(
            "RecordKind mismatch while trying to retrieve graph_entry from record {:?}",
            PrettyPrintRecordKey::from(&record.key)
        ))))
    }
}

fn deserialize_pointer_from_record(
    record: &Record,
) -> Result<ant_protocol::storage::Pointer, Error> {
    let key = &record.key;
    let header = RecordHeader::from_record(record).map_err(|err| {
        Error::GetError(GetError::Configuration(format!(
            "Failed to parse record header for pointer at {key:?}: {err:?}"
        )))
    })?;

    let kind = header.kind;
    if !matches!(kind, RecordKind::DataOnly(DataTypes::Pointer)) {
        error!("Record kind mismatch: expected Pointer, got {kind:?}");
        return Err(Error::GetError(GetError::RecordKindMismatch(
            RecordKind::DataOnly(DataTypes::Pointer),
        )));
    };

    let pointer: ant_protocol::storage::Pointer =
        try_deserialize_record(record).map_err(|err| {
            Error::GetError(GetError::Configuration(format!(
                "Failed to parse record for pointer at {key:?}: {err:?}"
            )))
        })?;

    Ok(pointer)
}

fn deserialize_scratchpad_from_record(
    record: &Record,
) -> Result<ant_protocol::storage::Scratchpad, Error> {
    try_deserialize_record::<ant_protocol::storage::Scratchpad>(record)
        .map_err(|e| Error::GetError(GetError::Protocol(e)))
}

fn select_highest_pointer_version(
    result_map: HashMap<PeerId, Record>,
    address: NetworkAddress,
) -> Result<ant_protocol::storage::Pointer, Error> {
    let highest_version = result_map
        .into_iter()
        .filter_map(
            |(peer, record)| match deserialize_pointer_from_record(&record) {
                Ok(pointer) => Some(pointer),
                Err(err) => {
                    warn!("Peer {peer:?} returned invalid pointer at {address} with error: {err}");
                    None
                }
            },
        )
        .max_by_key(|pointer| pointer.counter());

    match highest_version {
        Some(pointer) => Ok(pointer),
        None => {
            let msg = format!("Found multiple conflicting invalid pointers at {address}");
            warn!("{msg}");
            Err(Error::GetError(GetError::Configuration(msg)))
        }
    }
}

fn resolve_scratchpad_split(
    result_map: HashMap<PeerId, Record>,
    address: &NetworkAddress,
) -> Result<DataContent, Error> {
    let mut pads = result_map
        .values()
        .map(try_deserialize_record::<ant_protocol::storage::Scratchpad>)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| {
            Error::GetError(GetError::Configuration(format!(
                "Corrupt scratchpad at {address:?}"
            )))
        })?;

    // take the latest versions and filter out duplicates with same content
    pads.sort_by_key(|s| s.counter());
    let max_version = pads.last().map(|p| p.counter()).unwrap_or_else(|| {
        error!("Got empty scratchpad vector for {address:?}");
        u64::MAX
    });

    // Filter to latest version and remove duplicates with same content
    let latest_pads: Vec<_> = pads
        .into_iter()
        .filter(|s| s.counter() == max_version)
        .collect();

    // Remove duplicates
    let mut dedup_latest_pads = latest_pads.clone();
    dedup_latest_pads.dedup_by(|a, b| {
        a.data_encoding() == b.data_encoding() && a.encrypted_data() == b.encrypted_data()
    });

    // make sure we only have one of latest version
    match &dedup_latest_pads[..] {
        [one] => Ok(DataContent::Scratchpad(one.clone())),
        [] => {
            error!("Got no valid scratchpads for {address:?}");
            Err(Error::GetError(GetError::Configuration(format!(
                "Corrupt scratchpad at {address:?}"
            ))))
        }
        multi => {
            error!(
                "Got multiple conflicting scratchpads for {address:?} with the latest version: {latest_pads:?}"
            );
            Ok(DataContent::ScratchpadSplit(multi.to_vec()))
        }
    }
}

fn verify_pointer(pointer: &ant_protocol::storage::Pointer) -> Result<(), Error> {
    if !pointer.verify_signature() {
        return Err(Error::GetError(GetError::Configuration(
            "Pointer signature is invalid".to_string(),
        )));
    }
    Ok(())
}

fn verify_scratchpad(scratchpad: &ant_protocol::storage::Scratchpad) -> Result<(), Error> {
    if !scratchpad.verify_signature() {
        return Err(Error::GetError(GetError::Configuration(
            "Scratchpad signature is invalid".to_string(),
        )));
    }
    if scratchpad.is_too_big() {
        return Err(Error::GetError(GetError::Configuration(format!(
            "Scratchpad size is too big: {} > {}",
            scratchpad.size(),
            ant_protocol::storage::Scratchpad::MAX_SIZE
        ))));
    }
    Ok(())
}
