use crate::TX_TIMEOUT;
use crate::common::{Address, Calldata, TxHash};
use crate::transaction_config::{MaxFeePerGas, TransactionConfig};
use alloy::network::{Network, ReceiptResponse, TransactionBuilder};
use alloy::providers::{PendingTransactionBuilder, Provider};
use std::time::Duration;

pub(crate) const MAX_RETRIES: u8 = 3;
const DEFAULT_RETRY_INTERVAL_MS: u64 = 4000;
const BROADCAST_TRANSACTION_TIMEOUT_MS: u64 = 5000;
const WATCH_TIMEOUT_MS: u64 = 1000;

#[derive(thiserror::Error, Debug)]
pub enum TransactionError {
    #[error("Could not get current gas price: {0}")]
    CouldNotGetGasPrice(String),
    #[error("Gas price is above limit: {0}")]
    GasPriceAboveLimit(u128),
    #[error("Transaction failed to send: {0}")]
    TransactionFailedToSend(String),
    #[error("Transaction failed to confirm in time: {0}")]
    TransactionFailedToConfirm(String, Option<u64>), // Includes the nonce
    #[error("Transaction reverted with data")]
    TransactionReverted {
        message: String,
        revert_data: Option<alloy::primitives::Bytes>,
        nonce: Option<u64>,
    },
}

/// Execute an async closure that returns a result. Retry on failure.
pub(crate) async fn retry<F, Fut, T, E>(
    mut action: F,
    operation_id: &str,
    retry_interval_ms: Option<u64>,
) -> Result<T, E>
where
    F: FnMut() -> Fut + Send,
    Fut: std::future::Future<Output = Result<T, E>> + Send,
    E: std::fmt::Debug,
{
    let mut retries = 0;

    loop {
        match action().await {
            Ok(result) => return Ok(result),
            Err(err) => {
                if retries == MAX_RETRIES {
                    error!("{operation_id} failed after {retries} retries: {err:?}");
                    return Err(err);
                }

                retries += 1;
                let retry_interval_ms = retry_interval_ms.unwrap_or(DEFAULT_RETRY_INTERVAL_MS);
                let delay = Duration::from_millis(retry_interval_ms * retries.pow(2) as u64);

                warn!(
                    "Error trying {operation_id}: {err:?}. Retry #{retries} in {:?} second(s).",
                    delay.as_secs()
                );

                tokio::time::sleep(delay).await;
            }
        }
    }
}

/// Generic function to send a transaction with retries.
pub(crate) async fn send_transaction_with_retries<P, N>(
    provider: &P,
    calldata: Calldata,
    to: Address,
    tx_identifier: &str,
    transaction_config: &TransactionConfig,
) -> Result<TxHash, TransactionError>
where
    P: Provider<N>,
    N: Network,
{
    let mut previous_nonce: Option<u64> = None;
    let mut retries: u8 = 0;

    loop {
        match send_transaction(
            provider,
            calldata.clone(),
            to,
            previous_nonce,
            tx_identifier,
            transaction_config,
        )
        .await
        {
            Ok(tx_hash) => break Ok(tx_hash),
            Err(err) => {
                if retries == MAX_RETRIES {
                    error!(
                        "Transaction {tx_identifier} failed after {retries} retries. Giving up. Error: {err:?}"
                    );
                    break Err(err);
                }

                match err {
                    TransactionError::CouldNotGetGasPrice(reason) => {
                        warn!("Could not get gas price: {reason}");
                    }
                    TransactionError::GasPriceAboveLimit(limit) => {
                        warn!("Gas price is above limit: {limit}");
                    }
                    TransactionError::TransactionFailedToSend(reason) => {
                        warn!("Transaction failed to send: {reason}");
                    }
                    TransactionError::TransactionFailedToConfirm(reason, nonce) => {
                        warn!("Transaction failed to confirm: {reason} (nonce: {nonce:?})");
                        previous_nonce = nonce;
                    }
                    TransactionError::TransactionReverted {
                        ref message,
                        ref revert_data,
                        ref nonce,
                    } => {
                        warn!(
                            "Transaction reverted: {message} (nonce: {nonce:?}, has_data: {})",
                            revert_data.is_some()
                        );
                        // Don't retry on revert - the transaction will keep reverting
                        error!(
                            "Transaction {tx_identifier} reverted. Not retrying. Error: {message}"
                        );
                        break Err(err);
                    }
                }

                retries += 1;

                let retry_interval_ms = DEFAULT_RETRY_INTERVAL_MS;
                let delay = Duration::from_millis(retry_interval_ms * retries.pow(2) as u64);

                warn!(
                    "Retrying transaction (attempt {}) in {} second(s).",
                    retries,
                    delay.as_secs(),
                );

                tokio::time::sleep(delay).await;

                continue;
            }
        }
    }
}

async fn send_transaction<P, N>(
    provider: &P,
    calldata: Calldata,
    to: Address,
    mut nonce: Option<u64>,
    tx_identifier: &str,
    transaction_config: &TransactionConfig,
) -> Result<TxHash, TransactionError>
where
    P: Provider<N>,
    N: Network,
{
    let eip1559_fees = get_eip1559_fees(provider, transaction_config).await?;

    debug!("eip1559 fees: {eip1559_fees:?}");

    let mut transaction_request = provider
        .transaction_request()
        .with_to(to)
        .with_input(calldata.clone());

    if let Some(fees) = eip1559_fees {
        transaction_request.set_max_fee_per_gas(fees.max_fee_per_gas);
        transaction_request.set_max_priority_fee_per_gas(fees.max_priority_fee_per_gas);
    }

    // Estimate gas and add 20% buffer to avoid out-of-gas reverts
    // This is especially important for Arbitrum L2 where gas estimation can be tricky
    let estimated_gas = provider
        .estimate_gas(transaction_request.clone())
        .await
        .map_err(|e| {
            TransactionError::TransactionFailedToSend(format!("gas estimation failed: {e}"))
        })?;
    let gas_with_buffer = estimated_gas.saturating_mul(120) / 100;
    debug!("Estimated gas: {estimated_gas}, with 20% buffer: {gas_with_buffer}");
    transaction_request.set_gas_limit(gas_with_buffer);

    // Retry with the same nonce to replace a stuck transaction
    if let Some(nonce) = nonce {
        transaction_request.set_nonce(nonce);
    } else {
        nonce = transaction_request.nonce();
    }

    let pending_tx_builder_result = tokio::time::timeout(
        Duration::from_millis(BROADCAST_TRANSACTION_TIMEOUT_MS),
        provider.send_transaction(transaction_request.clone()),
    )
    .await;

    let pending_tx_builder = match pending_tx_builder_result {
        Ok(Ok(pending_tx_builder)) => pending_tx_builder,
        Ok(Err(err)) => return Err(TransactionError::TransactionFailedToSend(err.to_string())),
        Err(_) => {
            return Err(TransactionError::TransactionFailedToSend(
                "timeout".to_string(),
            ));
        }
    };

    debug!(
        "{tx_identifier} transaction is pending with tx_hash: {:?}",
        pending_tx_builder.tx_hash()
    );

    let watch_result = retry(
        || async {
            PendingTransactionBuilder::from_config(
                provider.root().clone(),
                pending_tx_builder.inner().clone(),
            )
            .with_timeout(Some(TX_TIMEOUT))
            .watch()
            .await
        },
        "watching pending transaction",
        Some(WATCH_TIMEOUT_MS),
    )
    .await;

    match watch_result {
        Ok(tx_hash) => {
            // CRITICAL: .watch() only confirms the tx was mined, NOT that it succeeded.
            // We must fetch the receipt and check the status field.
            match provider.get_transaction_receipt(tx_hash).await {
                Ok(Some(receipt)) => {
                    if receipt.status() {
                        debug!("{tx_identifier} transaction with hash {tx_hash:?} succeeded");
                        Ok(tx_hash)
                    } else {
                        // Transaction was mined but reverted (e.g., out of gas)
                        let gas_used = receipt.gas_used();
                        error!(
                            "{tx_identifier} transaction {tx_hash:?} was mined but reverted. \
                            Gas used: {gas_used}"
                        );
                        Err(TransactionError::TransactionReverted {
                            message: format!(
                                "Transaction was mined but execution failed (gas used: {gas_used})"
                            ),
                            revert_data: None,
                            nonce,
                        })
                    }
                }
                Ok(None) => {
                    // This shouldn't happen after .watch() succeeds, but handle it
                    warn!("{tx_identifier} transaction {tx_hash:?} receipt not found after watch");
                    Err(TransactionError::TransactionFailedToConfirm(
                        "Receipt not found after watch".to_string(),
                        nonce,
                    ))
                }
                Err(err) => {
                    error!("{tx_identifier} failed to get receipt for {tx_hash:?}: {err}");
                    Err(TransactionError::TransactionFailedToConfirm(
                        format!("Failed to get receipt: {err}"),
                        nonce,
                    ))
                }
            }
        }
        Err(err) => {
            // Try to get more details about the revert if available
            let revert_data = extract_revert_data(&err);

            if revert_data.is_some() {
                Err(TransactionError::TransactionReverted {
                    message: err.to_string(),
                    revert_data,
                    nonce,
                })
            } else {
                Err(TransactionError::TransactionFailedToConfirm(
                    err.to_string(),
                    nonce,
                ))
            }
        }
    }
}

/// Extract revert data from a PendingTransactionError
///
/// When a transaction reverts, we try to extract the revert reason data.
/// This data can then be decoded by contract-specific error handlers.
fn extract_revert_data(
    err: &alloy::providers::PendingTransactionError,
) -> Option<alloy::primitives::Bytes> {
    // Try to extract from the error message/data
    // Alloy encodes revert data in the error, we just need to find it
    let err_str = err.to_string();

    // Look for revert data in the error string
    // Format is usually "revert: <hex data>" or contains the error signature
    if err_str.contains("revert") || err_str.contains("0x") {
        // Try to extract hex data from the error
        // This is a simplified approach - in reality the data structure varies
        debug!("Transaction reverted: {}", err_str);
    }

    // For now, we'll return None and handle this at a higher level
    // by using eth_call to simulate the transaction when it fails
    None
}

/// EIP-1559 fee parameters for a transaction.
#[derive(Debug, Clone, Copy)]
struct Eip1559Fees {
    max_fee_per_gas: u128,
    max_priority_fee_per_gas: u128,
}

async fn get_eip1559_fees<P: Provider<N>, N: Network>(
    provider: &P,
    transaction_config: &TransactionConfig,
) -> Result<Option<Eip1559Fees>, TransactionError> {
    match transaction_config.max_fee_per_gas {
        MaxFeePerGas::Auto => {
            debug!("Using Auto mode for gas fees");
            // Use EIP-1559 fee estimation which includes a buffer for base fee fluctuation
            let eip1559_fees = provider
                .estimate_eip1559_fees()
                .await
                .map_err(|err| TransactionError::CouldNotGetGasPrice(err.to_string()))?;
            Ok(Some(Eip1559Fees {
                max_fee_per_gas: eip1559_fees.max_fee_per_gas,
                max_priority_fee_per_gas: eip1559_fees.max_priority_fee_per_gas,
            }))
        }
        MaxFeePerGas::LimitedAuto(limit) => {
            debug!("Using LimitedAuto mode for gas fees with limit: {limit}");
            // Use EIP-1559 fee estimation for better accuracy
            let eip1559_fees = provider
                .estimate_eip1559_fees()
                .await
                .map_err(|err| TransactionError::CouldNotGetGasPrice(err.to_string()))?;

            if eip1559_fees.max_fee_per_gas > limit {
                warn!(
                    "Estimated max_fee_per_gas ({}) exceeds limit ({})",
                    eip1559_fees.max_fee_per_gas, limit
                );
                Err(TransactionError::GasPriceAboveLimit(limit))
            } else {
                Ok(Some(Eip1559Fees {
                    max_fee_per_gas: eip1559_fees.max_fee_per_gas,
                    max_priority_fee_per_gas: eip1559_fees.max_priority_fee_per_gas,
                }))
            }
        }
        MaxFeePerGas::Custom(max_fee) => {
            debug!("Using Custom mode for gas fees with max_fee: {max_fee}");
            // Use custom max fee with estimated priority fee
            let eip1559_fees = provider
                .estimate_eip1559_fees()
                .await
                .map_err(|err| TransactionError::CouldNotGetGasPrice(err.to_string()))?;

            if max_fee < eip1559_fees.max_fee_per_gas {
                warn!(
                    "Custom max_fee_per_gas ({}) is below estimated fee ({}). Transaction may be slow or fail.",
                    max_fee, eip1559_fees.max_fee_per_gas
                );
            }

            Ok(Some(Eip1559Fees {
                max_fee_per_gas: max_fee,
                max_priority_fee_per_gas: eip1559_fees.max_priority_fee_per_gas,
            }))
        }
        MaxFeePerGas::Unlimited => {
            debug!("Using Unlimited mode for gas fees (no fee parameters will be set)");
            Ok(None)
        }
    }
}
