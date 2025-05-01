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
use std::sync::LazyLock;

/// Number of batch size of an entire quote-pay-upload flow to process.
/// This is mainly to avoid upload failure due to quote expiracy (1 hour).
/// Suggested to be the hourly throughput of the chunks can be handled.
///
/// Can be overridden by the `UPLOAD_FLOW_BATCH_SIZE` environment variable.
static UPLOAD_FLOW_BATCH_SIZE: LazyLock<usize> = LazyLock::new(|| {
    let batch_size = std::env::var("UPLOAD_FLOW_BATCH_SIZE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(50);
    info!("Upload flow batch size: {}", batch_size);
    batch_size
});

impl Client {
    /// Processes upload results and calculates total cost
    /// Returns total tokens spent or the first encountered upload error
    async fn calculate_total_cost(
        &self,
        upload_results: Vec<(String, Result<usize, UploadError>)>,
        payment_receipts: Vec<Receipt>,
        free_chunks_counts: Vec<usize>,
    ) -> Result<AttoTokens, UploadError> {
        // Process upload results and track errors
        let (total_uploaded, last_error) = upload_results.into_iter().fold(
            (0, None),
            |(mut total, mut last_err), (file_name, result)| {
                match result {
                    Ok(chunks_uploaded) => total += chunks_uploaded,
                    Err(err) => {
                        error!("Failed to upload file {file_name}: {err:?}");
                        #[cfg(feature = "loud")]
                        println!("Failed to upload file {file_name}: {err:?}");
                        last_err = Some(err);
                    }
                }
                (total, last_err)
            },
        );

        // Return early if any upload failed
        if let Some(err) = last_error {
            return Err(err);
        }

        // Calculate total tokens spent across all receipts
        let total_tokens: Amount = payment_receipts
            .into_iter()
            .flat_map(|receipt| receipt.into_values().map(|(_, cost)| cost.as_atto()))
            .sum();

        let total_free_chunks = free_chunks_counts.iter().sum::<usize>();

        // Send completion event if channel exists
        if let Some(sender) = &self.client_event_sender {
            let summary = UploadSummary {
                records_paid: total_uploaded.saturating_sub(total_free_chunks),
                records_already_paid: total_free_chunks,
                tokens_spent: total_tokens,
            };

            if let Err(err) = sender.send(ClientEvent::UploadComplete(summary)).await {
                error!("Failed to send upload completion event: {err:?}");
            }
        }

        Ok(AttoTokens::from_atto(total_tokens))
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
        let mut upload_results = Vec::with_capacity(total_files);
        let mut receipts = Vec::new();
        let mut free_chunks_counts = Vec::new();

        // Process each file's chunks in batches
        for ((file_name, data_address), mut chunks) in combined_chunks {
            info!("Processing file: {file_name} ({} chunks)", chunks.len());
            #[cfg(feature = "loud")]
            println!("Processing file: {file_name} ({} chunks)", chunks.len());

            // Process all chunks for this file in batches
            while !chunks.is_empty() {
                self.process_chunk_batch(
                    &file_name,
                    data_address,
                    &mut chunks,
                    &mut upload_results,
                    &mut receipts,
                    &mut free_chunks_counts,
                    payment_option.clone(),
                )
                .await?;
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

        self.calculate_total_cost(upload_results, receipts, free_chunks_counts)
            .await
    }

    /// Processes a single batch of chunks (quote -> pay -> upload)
    /// Returns error if any chunk in batch fails to upload
    #[allow(clippy::too_many_arguments)]
    async fn process_chunk_batch(
        &self,
        file_name: &str,
        data_address: Option<DataAddress>,
        remaining_chunks: &mut Vec<Chunk>,
        upload_results: &mut Vec<(String, Result<usize, UploadError>)>,
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
            .map(|chunk| (*chunk.name(), chunk.size()))
            .collect();

        info!("Processing batch of {} chunks", batch.len());
        #[cfg(feature = "loud")]
        println!("Processing batch of {} chunks", batch.len());

        // Process payment for this batch
        let (receipt, free_chunks) = self
            .pay_for_content_addrs(DataTypes::Chunk, payment_info.into_iter(), payment_option)
            .await
            .inspect_err(|err| error!("Payment failed: {err:?}"))
            .map_err(PutError::from)?;

        if free_chunks > 0 {
            info!("{free_chunks} chunks were free in this batch");
        }

        // Upload all chunks in batch with retries
        let mut failed_uploads = self
            .upload_chunks_with_retries(batch.iter().collect(), &receipt)
            .await;
        let successful_uploads = batch.len() - failed_uploads.len();

        // Handle upload results
        let result = match failed_uploads.pop() {
            Some((chunk, error)) => {
                error!("Failed to upload chunk ({:?}): {error:?}", chunk.address());
                (file_name.to_string(), Err(UploadError::from(error)))
            }
            None => {
                let destination = data_address
                    .as_ref()
                    .map(|addr| format!(" to: {}", hex::encode(addr.xorname())))
                    .unwrap_or_default();

                info!(
                    "Successfully uploaded {file_name} ({} chunks){destination}",
                    batch.len()
                );
                #[cfg(feature = "loud")]
                println!(
                    "Successfully uploaded {file_name} ({} chunks){destination}",
                    batch.len()
                );

                (file_name.to_string(), Ok(successful_uploads))
            }
        };

        // Store results
        upload_results.push(result);
        receipts.push(receipt);
        free_chunks_counts.push(free_chunks);

        Ok(())
    }
}
