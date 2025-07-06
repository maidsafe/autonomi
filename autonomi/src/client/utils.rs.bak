// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::client::PutError;
use futures::stream::{FuturesUnordered, StreamExt};
use std::future::Future;

pub(crate) async fn process_tasks_with_max_concurrency<I, R>(tasks: I, batch_size: usize) -> Vec<R>
where
    I: IntoIterator,
    I::Item: Future<Output = R> + Send,
    R: Send,
{
    let mut futures = FuturesUnordered::new();
    let mut results = Vec::new();

    for task in tasks.into_iter() {
        futures.push(task);

        if futures.len() >= batch_size {
            if let Some(result) = futures.next().await {
                results.push(result);
            }
        }
    }

    // Process remaining tasks
    while let Some(result) = futures.next().await {
        results.push(result);
    }

    results
}

/// Extracts gas fee values from an error message string.
///
/// Looks for patterns like "maxFeePerGas: <value>, baseFee: <value>" in the error string
/// and returns the extracted values as a tuple of (max_fee, base_fee) strings.
///
/// # Arguments
/// * `err_str` - The error string to parse
///
/// # Returns
/// * `Some((max_fee, base_fee))` if both values are found
/// * `None` if the pattern is not found or values cannot be extracted
pub(crate) fn extract_gas_values(err_str: &str) -> Option<(String, String)> {
    // Look for pattern: "maxFeePerGas: <value>, baseFee: <value>"
    if let Some(max_fee_start) = err_str.find("maxFeePerGas: ") {
        let max_fee_str = &err_str[max_fee_start + 14..];
        if let Some(comma_pos) = max_fee_str.find(',') {
            let max_fee = &max_fee_str[..comma_pos];

            if let Some(base_fee_start) = err_str.find("baseFee: ") {
                let base_fee_str = &err_str[base_fee_start + 9..];
                // Find the end of the base fee value (could be end of string or another delimiter)
                let base_fee = base_fee_str.split(|c: char| !c.is_numeric()).next()?;

                return Some((max_fee.to_string(), base_fee.to_string()));
            }
        }
    }
    None
}

/// Formats an upload error into a user-friendly error message.
///
/// This function analyzes the error type and returns an appropriate error message
/// with helpful information for the user.
///
/// # Arguments
/// * `err` - The upload error to format
///
/// # Returns
/// A formatted error message string with emojis and helpful suggestions
pub(crate) fn format_upload_error(err: &PutError) -> String {
    let err_str = format!("{err:?}");

    if err_str.contains("max fee per gas less than block base fee") {
        if let Some((max_fee, base_fee)) = extract_gas_values(&err_str) {
            format!(
                "❌ Gas fee too low!\n💰 Your max fee per gas: {max_fee} wei\n📈 Network base fee: {base_fee} wei\n💡 Increase your --max-fee-per-gas if you want the upload to be executed faster",
            )
        } else {
            "💸 Gas fee too low - current base fee exceeds your setting".to_string()
        }
    } else if err_str.contains("insufficient funds") {
        "💰 Insufficient funds for transaction".to_string()
    } else if let PutError::Batch(ref upload_state) = err {
        format!(
            "❌ Upload batch failed: {} chunks failed",
            upload_state.failed.len()
        )
    } else {
        "❌ Upload error occurred".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::ChunkBatchUploadState;

    #[test]
    fn test_extract_gas_values() {
        // Test successful extraction
        let err_str = "Error: max fee per gas less than block base fee: maxFeePerGas: 1000000000, baseFee: 2000000000";
        let result = extract_gas_values(err_str);
        assert_eq!(
            result,
            Some(("1000000000".to_string(), "2000000000".to_string()))
        );

        // Test with additional text after baseFee
        let err_str = "maxFeePerGas: 500, baseFee: 1000 (retry later)";
        let result = extract_gas_values(err_str);
        assert_eq!(result, Some(("500".to_string(), "1000".to_string())));

        // Test missing maxFeePerGas
        let err_str = "baseFee: 1000";
        let result = extract_gas_values(err_str);
        assert_eq!(result, None);

        // Test missing baseFee
        let err_str = "maxFeePerGas: 500";
        let result = extract_gas_values(err_str);
        assert_eq!(result, None);

        // Test empty string
        let err_str = "";
        let result = extract_gas_values(err_str);
        assert_eq!(result, None);
    }

    #[test]
    fn test_format_upload_error() {
        // Test batch error
        let mut batch_state = ChunkBatchUploadState::default();
        // Create dummy chunk addresses using XorName
        let chunk_addr1 = ant_protocol::storage::ChunkAddress::new(xor_name::XorName([1; 32]));
        let chunk_addr2 = ant_protocol::storage::ChunkAddress::new(xor_name::XorName([2; 32]));
        batch_state
            .failed
            .push((chunk_addr1, "test error".to_string()));
        batch_state
            .failed
            .push((chunk_addr2, "test error 2".to_string()));
        let batch_err = PutError::Batch(batch_state);
        let err_msg = format_upload_error(&batch_err);
        assert_eq!(err_msg, "❌ Upload batch failed: 2 chunks failed");

        // Test generic error (we can't easily construct the exact Network error)
        // So we'll test with a simpler error case
        let generic_err = PutError::Batch(ChunkBatchUploadState::default());
        let err_msg = format_upload_error(&generic_err);
        assert_eq!(err_msg, "❌ Upload batch failed: 0 chunks failed");
    }
}
