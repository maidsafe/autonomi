// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use super::CombinedChunks;
use crate::client::high_level::data::DataAddress;
use crate::client::payment::PaymentOption;
use crate::client::payment::Receipt;
use crate::client::{ClientEvent, PutError, UploadSummary};
use crate::files::UploadError;
use crate::Client;
use ant_evm::{Amount, AttoTokens};
use ant_protocol::storage::{Chunk, DataTypes};
use evmlib::contract::payment_vault::MAX_TRANSFERS_PER_TRANSACTION;
use std::sync::LazyLock;

type AggregatedChunks = Vec<((String, Option<DataAddress>, usize, usize), Chunk)>;

/// Number of batch size of an entire quote-pay-upload flow to process.
/// Suggested to be multiples of `MAX_TRANSFERS_PER_TRANSACTION  / 3` (records-payouts-per-transaction).
///
/// Can be overridden by the `UPLOAD_FLOW_BATCH_SIZE` environment variable.
static UPLOAD_FLOW_BATCH_SIZE: LazyLock<usize> = LazyLock::new(|| {
    let batch_size = std::env::var("UPLOAD_FLOW_BATCH_SIZE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(MAX_TRANSFERS_PER_TRANSACTION / 3);
    info!("Upload flow batch size: {}", batch_size);
    batch_size
});

impl Client {
    /// Returns total tokens spent or the first encountered upload error
    async fn calculate_total_cost(
        &self,
        total_chunks: usize,
        payment_receipts: Vec<Receipt>,
        free_chunks_counts: Vec<usize>,
    ) -> AttoTokens {
        // Calculate total tokens spent across all receipts
        let total_tokens: Amount = payment_receipts
            .into_iter()
            .flat_map(|receipt| receipt.into_values().map(|(_, cost)| cost.as_atto()))
            .sum();

        let total_free_chunks = free_chunks_counts.iter().sum::<usize>();

        // Send completion event if channel exists
        if let Some(sender) = &self.client_event_sender {
            let summary = UploadSummary {
                records_paid: total_chunks.saturating_sub(total_free_chunks),
                records_already_paid: total_free_chunks,
                tokens_spent: total_tokens,
            };

            if let Err(err) = sender.send(ClientEvent::UploadComplete(summary)).await {
                error!("Failed to send upload completion event: {err:?}");
            }
        }

        AttoTokens::from_atto(total_tokens)
    }

    /// Processes file uploads with payment in batches
    /// Returns total cost of uploads or error if any upload fails
    pub(crate) async fn pay_and_upload(
        &self,
        payment_option: PaymentOption,
        combined_chunks: CombinedChunks,
    ) -> Result<AttoTokens, UploadError> {
        let start = tokio::time::Instant::now();
        let total_files = combined_chunks.len();
        let mut receipts = Vec::new();
        let mut free_chunks_counts = Vec::new();

        // Process all combined chunks in batches.
        // zip the file infos together for the better batch progressing print out.
        let mut aggregated_chunks = vec![];
        for ((file_name, data_address), chunks) in combined_chunks {
            let total = chunks.len();
            for (i, chunk) in chunks.into_iter().enumerate() {
                aggregated_chunks.push(((file_name.clone(), data_address, i, total), chunk));
            }
        }

        info!(
            "Processing total {} chunks of {total_files} files",
            aggregated_chunks.len()
        );
        #[cfg(feature = "loud")]
        println!(
            "Processing total {} chunks of {total_files} files",
            aggregated_chunks.len()
        );

        let total_chunks = aggregated_chunks.len();

        // Process all chunks for this file in batches
        while !aggregated_chunks.is_empty() {
            self.process_chunk_batch(
                &mut aggregated_chunks,
                &mut receipts,
                &mut free_chunks_counts,
                payment_option.clone(),
            )
            .await?;
        }

        info!(
            "Upload of {total_files} files completed in {:?}",
            start.elapsed()
        );
        #[cfg(feature = "loud")]
        println!(
            "Upload of {total_files} files completed in {:?}",
            start.elapsed()
        );

        Ok(self
            .calculate_total_cost(total_chunks, receipts, free_chunks_counts)
            .await)
    }

    /// Processes a single batch of chunks (quote -> pay -> upload)
    /// Returns error if any chunk in batch fails to upload
    #[allow(clippy::too_many_arguments)]
    async fn process_chunk_batch(
        &self,
        remaining_chunks: &mut AggregatedChunks,
        receipts: &mut Vec<Receipt>,
        free_chunks_counts: &mut Vec<usize>,
        payment_option: PaymentOption,
    ) -> Result<(), UploadError> {
        // Take next batch of chunks (up to UPLOAD_FLOW_BATCH_SIZE)
        let batch: Vec<_> = remaining_chunks
            .drain(..std::cmp::min(remaining_chunks.len(), *UPLOAD_FLOW_BATCH_SIZE))
            .collect();

        // Prepare payment info for batch
        let payment_info: Vec<_> = batch
            .iter()
            .map(|(_, chunk)| (*chunk.name(), chunk.size()))
            .collect();

        info!("Processing batch of {} chunks", batch.len());
        #[cfg(feature = "loud")]
        println!("Processing batch of {} chunks", batch.len());

        let mut file_infos = vec![];
        let mut batch_chunks = vec![];

        for (chunk_info, chunk) in batch {
            file_infos.push(chunk_info);
            batch_chunks.push(chunk);
        }

        for (file_name, file_addr, i, total) in file_infos.iter() {
            // File won't have address info if uploaded as private,
            // hence using different output messaging to avoid confusion.
            let output_str = if let Some(addr) = file_addr {
                format!(
                    "Processing chunk ({}/{total}) of {file_name:?} at {addr:?}",
                    i + 1
                )
            } else {
                format!("Processing chunk ({}/{total}) of {file_name:?}", i + 1)
            };
            info!("{output_str}");
            #[cfg(feature = "loud")]
            println!("{output_str}");
        }

        // Process payment for this batch
        let (receipt, free_chunks) = self
            .pay_for_content_addrs(DataTypes::Chunk, payment_info.into_iter(), payment_option)
            .await
            .inspect_err(|err| error!("Payment failed: {err:?}"))
            .map_err(PutError::from)?;

        if free_chunks > 0 {
            info!(
                "{free_chunks} chunks were free in this batch {}",
                batch_chunks.len()
            );
            #[cfg(feature = "loud")]
            println!(
                "{free_chunks} chunks were free in this batch {}",
                batch_chunks.len()
            );
        }

        // Upload all chunks in batch with retries
        self.chunk_batch_upload(batch_chunks.iter().collect(), &receipt)
            .await?;

        receipts.push(receipt);
        free_chunks_counts.push(free_chunks);

        Ok(())
    }
}
