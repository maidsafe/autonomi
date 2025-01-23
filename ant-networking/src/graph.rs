// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::config::GetRecordCfg;
use crate::{Network, NetworkError, Result};
use ant_protocol::storage::{DataTypes, GraphEntry, GraphEntryAddress};
use ant_protocol::{
    storage::{try_deserialize_record, RecordHeader, RecordKind},
    NetworkAddress, PrettyPrintRecordKey,
};
use libp2p::kad::Record;

impl Network {
    /// Gets GraphEntry at GraphEntryAddress from the Network.
    pub async fn get_graph_entry(
        &self,
        address: GraphEntryAddress,
        get_cfg: GetRecordCfg,
    ) -> Result<Vec<GraphEntry>> {
        let key = NetworkAddress::from_graph_entry_address(address).to_record_key();

        let record = self.get_record_from_network(key.clone(), &get_cfg).await?;
        debug!(
            "Got record from the network, {:?}",
            PrettyPrintRecordKey::from(&record.key)
        );

        get_graph_entry_from_record(&record)
    }
}

pub fn get_graph_entry_from_record(record: &Record) -> Result<Vec<GraphEntry>> {
    let header = RecordHeader::from_record(record)?;
    if let RecordKind::DataOnly(DataTypes::GraphEntry) = header.kind {
        let entry = try_deserialize_record::<Vec<GraphEntry>>(record)?;
        Ok(entry)
    } else {
        warn!(
            "RecordKind mismatch while trying to retrieve graph_entry from record {:?}",
            PrettyPrintRecordKey::from(&record.key)
        );
        Err(NetworkError::RecordKindMismatch(RecordKind::DataOnly(
            DataTypes::GraphEntry,
        )))
    }
}
