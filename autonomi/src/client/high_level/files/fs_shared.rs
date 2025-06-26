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
    /// Will try to carry out retry if `retry_failed` configured
    /// Returns total cost of uploads or error, once completed or cann't recover from failures
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

        let mut processed_chunks = 0;
        // Limited level of retry is turned on by default
        let mut retry_on_failure = true;
        let allowed_attempts =
            total_chunks + std::cmp::max(20, total_chunks * self.retry_failed as usize);

        // Process all chunks for this file in batches
        while !aggregated_chunks.is_empty() {
            // Take next batch of chunks (up to UPLOAD_FLOW_BATCH_SIZE)
            let batch_chunks: Vec<_> = aggregated_chunks
                .drain(..std::cmp::min(aggregated_chunks.len(), *UPLOAD_FLOW_BATCH_SIZE))
                .collect();
            let candidate_chunks = batch_chunks.len();

            let (retry_chunks, receipt, free_chunks_count, upload_error) = self
                .process_chunk_batch(batch_chunks, payment_option.clone(), retry_on_failure)
                .await;
            receipts.extend(receipt);
            free_chunks_counts.extend(free_chunks_count);
            if let Some(err) = upload_error {
                return Err(err);
            }

            // If retry_failed, tracking the processed_chunks.
            // Flip the flag once max_allownce hit to terminate the flow.
            if retry_on_failure {
                processed_chunks += std::cmp::min(candidate_chunks, *UPLOAD_FLOW_BATCH_SIZE);

                if processed_chunks > allowed_attempts {
                    retry_on_failure = false;
                }
            }

            if !retry_chunks.is_empty() {
                // there was upload failure happens, in that case, carry out a short sleep
                // to allow the glitch calm down.
                println!("⚠️  Encountered upload failure, take 1 minute pause before continue...");
                info!("Encountered upload failure, take 1 minute pause before continue...");

                // Wait 1 minute before retry
                sleep(Duration::from_secs(60)).await;
                println!("🔄 continue with upload...");
                info!("🔄 continue with upload...");
            }
            aggregated_chunks.extend(retry_chunks);
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
    /// Returns: (failed_chunks_for_retry, receipt, free_chunks_counts, error_if_retry_on_failure_not_enabled)
    #[allow(clippy::too_many_arguments)]
    async fn process_chunk_batch(
        &self,
        mut batch: AggregatedChunks,
        payment_option: PaymentOption,
        retry_on_failure: bool,
    ) -> (
        AggregatedChunks,
        Vec<Receipt>,
        Vec<usize>,
        Option<UploadError>,
    ) {
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
        let mut upload_error = None;

        for (chunk_info, chunk) in batch.clone() {
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
        let (receipt, free_chunks) = match self
            .pay_for_content_addrs(DataTypes::Chunk, payment_info.into_iter(), payment_option)
            .await
        {
            Ok((receipt, free_chunks)) => (receipt, free_chunks),
            Err(err) => {
                if retry_on_failure {
                    info!("Quoting or payment error encountered, retry scheduled {err:?}");
                    println!("Quoting or payment error encountered, retry scheduled.");
                    return (batch, vec![], vec![], None);
                } else {
                    return (
                        vec![],
                        vec![],
                        vec![],
                        Some(UploadError::from(PutError::from(err))),
                    );
                }
            }
        };

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

        // Upload all chunks in batch, schedule failed_chunks for retry (if retry_failed set)
        let mut retry_chunks = vec![];
        match self
            .chunk_batch_upload(batch_chunks.iter().collect(), &receipt)
            .await
        {
            // No upload failure encountered
            Ok(()) => {}
            Err(err) if retry_on_failure => {
                // Format error message for user
                let error_msg = format_upload_error(&err);
                println!("⚠️  {error_msg}. Retrying scheduled");
                info!("Upload error: {err}. Retrying scheduled");

                if let PutError::Batch(ref upload_state) = err {
                    let failed_chunks: Vec<_> =
                        upload_state.failed.iter().map(|(addr, _)| *addr).collect();
                    // Filter out failed entries
                    batch.retain(|(_, chunk)| failed_chunks.contains(chunk.address()));
                    // Push back failed entries
                    retry_chunks.extend(batch);
                } else {
                    // Encounterred Un-recoverable upload errors
                    // Return immediately to terminate the entire upload flow
                    upload_error = Some(UploadError::PutError(err));
                };
            }
            Err(err) => upload_error = Some(UploadError::PutError(err)),
        }

        (retry_chunks, vec![receipt], vec![free_chunks], upload_error)
    }
}
