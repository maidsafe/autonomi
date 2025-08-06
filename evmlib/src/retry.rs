use crate::TX_TIMEOUT;
use crate::common::{Address, Calldata, TxHash};
use crate::transaction_config::{MaxFeePerGas, TransactionConfig};
use alloy::network::{Network, TransactionBuilder};
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
    let max_fee_per_gas = get_max_fee_per_gas(provider, transaction_config).await?;

    debug!("max fee per gas: {max_fee_per_gas:?}");

    let mut transaction_request = provider
        .transaction_request()
        .with_to(to)
        .with_input(calldata.clone());

    if let Some(max_fee_per_gas) = max_fee_per_gas {
        transaction_request.set_max_fee_per_gas(max_fee_per_gas);
    }

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
            debug!("{tx_identifier} transaction with hash {tx_hash:?} is successful");
            Ok(tx_hash)
        }
        Err(err) => Err(TransactionError::TransactionFailedToConfirm(
            err.to_string(),
            nonce,
        )),
    }
}

async fn get_max_fee_per_gas<P: Provider<N>, N: Network>(
    provider: &P,
    transaction_config: &TransactionConfig,
) -> Result<Option<u128>, TransactionError> {
    match transaction_config.max_fee_per_gas {
        MaxFeePerGas::Auto => provider
            .get_gas_price()
            .await
            .map(Some)
            .map_err(|err| TransactionError::CouldNotGetGasPrice(err.to_string())),
        MaxFeePerGas::LimitedAuto(limit) => {
            let gas_price = provider
                .get_gas_price()
                .await
                .map_err(|err| TransactionError::CouldNotGetGasPrice(err.to_string()))?;

            if gas_price > limit {
                Err(TransactionError::GasPriceAboveLimit(limit))
            } else {
                Ok(Some(gas_price))
            }
        }
        MaxFeePerGas::Custom(wei) => Ok(Some(wei)),
        MaxFeePerGas::Unlimited => Ok(None),
    }
}
