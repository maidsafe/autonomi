// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::Client;
use crate::client::config::UPLOAD_FLOW_BATCH_SIZE;
use crate::client::payment::PayError::EvmWalletError;
use crate::client::payment::PaymentOption;
use crate::client::payment::Receipt;
use crate::client::{ClientEvent, PutError, UploadSummary};
use crate::self_encryption::EncryptionStream;
use crate::utils::format_upload_error;
use ant_evm::{Amount, AttoTokens};
use ant_protocol::storage::{Chunk, DataTypes};
use evmlib::wallet::Error::InsufficientTokensForQuotes;
use std::time::Duration;
use tokio::time::sleep;

type AggregatedChunks = Vec<((String, usize, usize), Chunk)>;

impl Client {
    /// Returns total tokens spent or the first encountered upload error
    pub(crate) async fn calculate_total_cost(
        &self,
        total_chunks: usize,
        payment_receipts: Vec<Receipt>,
        total_free_chunks: usize,
    ) -> AttoTokens {
        // Calculate total tokens spent across all receipts
        let total_tokens: Amount = payment_receipts
            .into_iter()
            .flat_map(|receipt| receipt.into_values().map(|(_, cost)| cost.as_atto()))
            .sum();

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
        encryption_streams: &mut [EncryptionStream],
    ) -> Result<AttoTokens, PutError> {
        let start = tokio::time::Instant::now();
        let total_files = encryption_streams.len();
        let mut receipts = Vec::new();
        let mut total_free_chunks = 0;
        let mut total_chunks = 0;

        // Estimate total chunks to be processed
        let maybe_file = if total_files > 1 {
            &format!(" of {total_files} files")
        } else {
            ""
        };
        let est_total_chunks: usize = encryption_streams
            .iter()
            .map(|stream| stream.total_chunks())
            .sum();
        info!("Processing estimated total {est_total_chunks} chunks{maybe_file}");
        #[cfg(feature = "loud")]
        println!("Processing estimated total {est_total_chunks} chunks{maybe_file}");

        // Process to upload file by file
        for stream in encryption_streams.iter_mut() {
            if !stream.file_path.is_empty() {
                info!("Uploading file: {}", stream.file_path);
                #[cfg(feature = "loud")]
                println!("Uploading file: {}", stream.file_path);
            }
            let (processed_chunks, free_chunks, receipt) = self
                .pay_and_upload_file(payment_option.clone(), stream)
                .await?;
            total_chunks += processed_chunks;
            total_free_chunks += free_chunks;
            receipts.extend(receipt);

            // Report upload completion
            let filename = stream.file_path.clone();
            let addr_if_pub = stream
                .data_address()
                .map(|addr| format!(" at {}", addr.to_hex()))
                .unwrap_or_else(|| "".to_string());
            let filename_if_any = if !filename.is_empty() {
                &format!(" for file {filename}")
            } else {
                ""
            };
            info!("Upload completed{filename_if_any}{addr_if_pub}");
            #[cfg(feature = "loud")]
            println!("Upload completed{filename_if_any}{addr_if_pub}");
        }

        // Report
        let total_elapsed = start.elapsed();
        info!("Upload{maybe_file} completed in {total_elapsed:?}");
        #[cfg(feature = "loud")]
        println!("Upload{maybe_file} completed in {total_elapsed:?}");

        Ok(self
            .calculate_total_cost(total_chunks, receipts, total_free_chunks)
            .await)
    }

    /// Returns: (processed_chunks, total_free_chunks, receipt)
    pub(crate) async fn pay_and_upload_file(
        &self,
        payment_option: PaymentOption,
        file: &mut EncryptionStream,
    ) -> Result<(usize, usize, Vec<Receipt>), PutError> {
        let est_total_todo = file.total_chunks();
        let mut processed_chunks = 0;
        let mut total_free_chunks = 0;
        let mut receipts = vec![];

        // Allow up to `retry_failed` * est_total_chunks total uploads to be attempted
        let mut retry_on_failure = true;
        let mut attempted_uploads = 0;
        let allowed_attempts =
            est_total_todo + std::cmp::max(20, est_total_todo * self.retry_failed as usize);

        // Process all chunks for this file in batches
        let mut current_batch = vec![];

        loop {
            // Get next batch if current_batch has space and file has more chunks
            if let Some(next_batch) = file.next_batch(*UPLOAD_FLOW_BATCH_SIZE - current_batch.len())
            {
                // prepare batch
                let next_batch_len = next_batch.len();
                let path = file.file_path.clone();
                let aggr_batch: AggregatedChunks = next_batch
                    .into_iter()
                    .enumerate()
                    .map(|(i, chunk)| ((path.clone(), processed_chunks + i, est_total_todo), chunk))
                    .collect();
                current_batch.extend(aggr_batch);

                // process batch
                processed_chunks += next_batch_len;
            }

            // If we have no chunks to process, break the loop
            if current_batch.is_empty() {
                break;
            }

            attempted_uploads += current_batch.len();

            let (retry_chunks, receipt, free_chunks_count, put_error) = self
                .process_chunk_batch(current_batch, payment_option.clone(), retry_on_failure)
                .await;

            receipts.extend(receipt);
            total_free_chunks += free_chunks_count;

            if let Some(err) = put_error {
                return Err(err);
            }

            // retry failed chunks
            if !retry_chunks.is_empty() {
                if attempted_uploads > allowed_attempts {
                    retry_on_failure = false;
                }

                // there was upload failure happens, in that case, carry out a short sleep
                // to allow the glitch calm down.
                #[cfg(feature = "loud")]
                println!("⚠️ Encountered upload failure, take 1 minute pause before continue...");
                info!("Encountered upload failure, take 1 minute pause before continue...");

                // Wait 1 minute before retry
                sleep(Duration::from_secs(60)).await;
                #[cfg(feature = "loud")]
                println!("🔄 continue with upload...");
                info!("🔄 continue with upload...");
            }

            current_batch = retry_chunks;
        }

        Ok((processed_chunks, total_free_chunks, receipts))
    }

    /// Processes a single batch of chunks (quote -> pay -> upload)
    /// Returns: (failed_chunks_for_retry, receipt, free_chunks_counts, error_if_retry_on_failure_not_enabled)
    #[allow(clippy::too_many_arguments)]
    async fn process_chunk_batch(
        &self,
        mut batch: AggregatedChunks,
        payment_option: PaymentOption,
        retry_on_failure: bool,
    ) -> (AggregatedChunks, Vec<Receipt>, usize, Option<PutError>) {
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
        let mut put_error = None;

        for (chunk_info, chunk) in batch.clone() {
            file_infos.push(chunk_info);
            batch_chunks.push(chunk);
        }

        for (file_name, i, est_total) in file_infos.iter() {
            let maybe_file = if !file_name.is_empty() {
                &format!(" of {file_name}")
            } else {
                ""
            };
            info!("Processing chunk ({}/{est_total}){maybe_file}", i + 1);
            #[cfg(feature = "loud")]
            println!("Processing chunk ({}/{est_total}){maybe_file}", i + 1);
        }

        // Process payment for this batch
        let (receipt, free_chunks) = match self
            .pay_for_content_addrs(DataTypes::Chunk, payment_info.into_iter(), payment_option)
            .await
        {
            Ok((receipt, free_chunks)) => (receipt, free_chunks),
            Err(err) if matches!(err, EvmWalletError(InsufficientTokensForQuotes(_, _))) => {
                error!("Insufficient tokens: {err:?}. Returning immediately.");
                return (vec![], vec![], 0, Some(PutError::from(err)));
            }
            Err(err) => {
                return if retry_on_failure {
                    error!("Quoting or payment error encountered, retry scheduled {err:?}");
                    println!("Quoting or payment error encountered, retry scheduled.");
                    (batch, vec![], 0, None)
                } else {
                    error!("Quoting or payment error encountered, no retry scheduled {err:?}");
                    (vec![], vec![], 0, Some(PutError::from(err)))
                };
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
                    put_error = Some(err);
                };
            }
            Err(err) => put_error = Some(err),
        }

        (retry_chunks, vec![receipt], free_chunks, put_error)
    }
}
