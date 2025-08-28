// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

pub use ant_protocol::storage::{GraphContent, GraphEntry, GraphEntryAddress};
pub use bls::{PublicKey, SecretKey};

use crate::{GetError, networking::Record};

use ant_protocol::{
    PrettyPrintRecordKey,
    storage::{DataTypes, RecordHeader, RecordKind, try_deserialize_record, try_serialize_record},
};

/// Deserialize graphentries from fetched record
/// in case split happens, raise a split error with all entries for the caller to resolve further
pub(crate) fn deserialize_graph_entry_from_record(record: &Record) -> Result<GraphEntry, GetError> {
    let pretty_addr = PrettyPrintRecordKey::from(&record.key);
    let header = RecordHeader::from_record(record).map_err(|_| {
        GetError::Configuration(format!(
            "Failed to deserialize record header {pretty_addr:?}",
        ))
    })?;

    if let RecordKind::DataOnly(DataTypes::GraphEntry) = header.kind {
        let entries = try_deserialize_record::<Vec<GraphEntry>>(record).map_err(|_| {
            GetError::Configuration(format!(
                "Failed to deserialize record value {pretty_addr:?}",
            ))
        })?;

        match &entries[..] {
            [entry] => Ok(entry.clone()),
            [] => {
                error!("Got no valid graphentry for {pretty_addr:?}");
                Err(GetError::Configuration(format!(
                    "Corrupt graphentry at {pretty_addr:?}"
                )))
            }
            multiple => {
                warn!(
                    "Graph entry fork detected at {pretty_addr:?}, returning all {} entries",
                    multiple.len()
                );
                Err(GetError::SplitRecord(
                    multiple
                        .iter()
                        .filter_map(|entry| {
                            if let Ok(bytes) = try_serialize_record(
                                entry,
                                RecordKind::DataOnly(DataTypes::GraphEntry),
                            ) {
                                Some(Record {
                                    key: record.key.clone(),
                                    value: bytes.to_vec(),
                                    publisher: None,
                                    expires: None,
                                })
                            } else {
                                None
                            }
                        })
                        .collect(),
                ))
            }
        }
    } else {
        warn!(
            "RecordKind mismatch while trying to retrieve graph_entry from record {pretty_addr:?}",
        );
        Err(GetError::Configuration(format!(
            "RecordKind mismatch while trying to retrieve graph_entry from record {pretty_addr:?}",
        )))
    }
}
