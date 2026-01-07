// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use ant_protocol::NetworkAddress;
use autonomi::PublicKey;
use autonomi::client::data_types::chunk::ChunkAddress;
use autonomi::client::data_types::graph::GraphEntryAddress;
use autonomi::client::{Amount, ClientEvent, UploadSummary};
use autonomi::networking::PeerId;
use color_eyre::Result;

/// Parse a string into a NetworkAddress.
///
/// Accepts:
/// - ChunkAddress (hex)
/// - PublicKey (hex) - for GraphEntry, Pointer, or Scratchpad addresses
/// - Raw 32-byte hex (XorName)
/// - PeerId
/// - NetworkAddress debug format (e.g., `NetworkAddress::RecordKey("...")`)
pub fn parse_network_address(addr: &str) -> Result<NetworkAddress> {
    let hex_str = addr.strip_prefix("0x").unwrap_or(addr);

    // Try parsing as ChunkAddress first
    if let Ok(chunk_addr) = ChunkAddress::from_hex(addr) {
        return Ok(NetworkAddress::from(chunk_addr));
    }

    // Try parsing as PublicKey (could be GraphEntry, Pointer, or Scratchpad)
    if let Ok(public_key) = PublicKey::from_hex(hex_str) {
        return Ok(NetworkAddress::from(GraphEntryAddress::new(public_key)));
    }

    // Try parsing from NetworkAddress debug format:
    // NetworkAddress::RecordKey("e9d7b3208bcb7ef566102027ca9a7f3ced7c0f8abf87c9bb0ef9130b625572f2") - (...)
    if let Some(start) = addr.find('"')
        && let Some(end) = addr[start + 1..].find('"')
    {
        let extracted_hex = &addr[start + 1..start + 1 + end];
        if let Ok(chunk_addr) = ChunkAddress::from_hex(extracted_hex) {
            return Ok(NetworkAddress::from(chunk_addr));
        }
    }

    // Try to parse as raw hex bytes (xor_name)
    if let Ok(bytes) = hex::decode(hex_str)
        && bytes.len() == 32
    {
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        return Ok(NetworkAddress::from(xor_name::XorName(arr)));
    }

    // Try to parse as a PeerId
    if let Ok(peer_id) = addr.parse::<PeerId>() {
        return Ok(NetworkAddress::from(peer_id));
    }

    Err(color_eyre::eyre::eyre!(
        "Could not parse address. Expected ChunkAddress, PublicKey, XorName, PeerId, or NetworkAddress debug format. Got: {addr}"
    ))
}

/// Collects upload summary from the event receiver.
/// Send a signal to the returned sender to stop collecting and to return the result via the join handle.
pub fn collect_upload_summary(
    mut event_receiver: tokio::sync::mpsc::Receiver<ClientEvent>,
) -> (
    tokio::task::JoinHandle<UploadSummary>,
    tokio::sync::oneshot::Sender<()>,
) {
    let (upload_completed_tx, mut upload_completed_rx) = tokio::sync::oneshot::channel::<()>();
    let stats_thread = tokio::spawn(async move {
        let mut tokens_spent: Amount = Amount::from(0);
        let mut record_count = 0;
        let mut records_already_paid = 0;

        loop {
            tokio::select! {
                event = event_receiver.recv() => {
                    match event {
                        Some(ClientEvent::UploadComplete(upload_summary)) => {
                            tokens_spent += upload_summary.tokens_spent;
                            record_count += upload_summary.records_paid;
                            records_already_paid += upload_summary.records_already_paid;
                        }
                        None => break,
                    }
                }
                _ = &mut upload_completed_rx => break,
            }
        }

        // try to drain the event receiver in case there are any more events
        while let Ok(event) = event_receiver.try_recv() {
            match event {
                ClientEvent::UploadComplete(upload_summary) => {
                    tokens_spent += upload_summary.tokens_spent;
                    record_count += upload_summary.records_paid;
                    records_already_paid += upload_summary.records_already_paid;
                }
            }
        }

        UploadSummary {
            tokens_spent,
            records_paid: record_count,
            records_already_paid,
        }
    });

    (stats_thread, upload_completed_tx)
}
