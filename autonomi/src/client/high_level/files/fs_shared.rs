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
use crate::client::utils::format_upload_error;
use crate::client::{ClientEvent, PutError, UploadSummary};
use crate::files::UploadError;
use crate::Client;
use ant_evm::{Amount, AttoTokens};
use ant_protocol::storage::{Chunk, DataTypes};
use evmlib::contract::payment_vault::MAX_TRANSFERS_PER_TRANSACTION;
use std::sync::LazyLock;
use std::time::Duration;
use tokio::time::sleep;

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
    pub(crate) async fn pay_and_upload(
        &self,
        payment_option: PaymentOption,
        combined_chunks: CombinedChunks,
    ) -> Result<AttoTokens, UploadError> {
        self.pay_and_upload_internal(payment_option, combined_chunks, self.retry_failed).await
    }

    /// Internal method that handles the actual upload logic with optional retry
    async fn pay_and_upload_internal(
        &self,
        payment_option: PaymentOption,
        combined_chunks: CombinedChunks,
        retry_on_failure: bool,
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
            let batch_result = self.process_chunk_batch(
                &mut aggregated_chunks,
                &mut receipts,
                &mut free_chunks_counts,
                payment_option.clone(),
                retry_on_failure,
            )
            .await;

            match batch_result {
                Ok(()) => continue,
                Err(err) if retry_on_failure => {
                    // Format error message for user
                    let error_msg = format_upload_error(&err);
                    
                    println!("⚠️  {}. Retrying after 1 minute pause...", error_msg);
                    info!("Upload error: {}. Retrying in 1 minute...", err);
                    
                    // Wait 1 minute before retry
                    sleep(Duration::from_secs(60)).await;
                    println!("🔄 Retrying upload...");
                    
                    // Continue the loop to retry with the same chunks
                    continue;
                }
                Err(err) => return Err(err),
            }
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
        aggregated_chunks: &mut AggregatedChunks,
        receipts: &mut Vec<Receipt>,
        free_chunks_counts: &mut Vec<usize>,
        payment_option: PaymentOption,
        retry_on_failure: bool,
    ) -> Result<(), UploadError> {
        // Take next batch of chunks (up to UPLOAD_FLOW_BATCH_SIZE)
        let batch_size = std::cmp::min(aggregated_chunks.len(), *UPLOAD_FLOW_BATCH_SIZE);
        
        // Important: Don't drain chunks yet - we might need to retry them
        let batch: Vec<_> = if retry_on_failure {
            // For retry mode, clone the chunks so we can retry if needed
            aggregated_chunks[..batch_size]
                .iter()
                .cloned()
                .collect()
        } else {
            // For non-retry mode, drain as before
            aggregated_chunks
                .drain(..batch_size)
                .collect()
        };

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
            .map_err(|err| UploadError::from(PutError::from(err)))?;

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

        // Only remove chunks from aggregated_chunks if retry mode and upload was successful
        if retry_on_failure {
            aggregated_chunks.drain(..batch_size);
        }

        Ok(())
    }
}
