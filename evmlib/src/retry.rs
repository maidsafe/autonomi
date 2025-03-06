use crate::common::{Address, Calldata, TxHash};
use crate::transaction_config::TransactionConfig;
use crate::TX_TIMEOUT;
use alloy::network::{Network, TransactionBuilder};
use alloy::providers::{PendingTransactionBuilder, Provider};
use alloy::transports::Transport;
use std::time::Duration;

pub(crate) const MAX_RETRIES: u8 = 3;
const DEFAULT_RETRY_INTERVAL_MS: u64 = 4000;
const BROADCAST_TRANSACTION_TIMEOUT_MS: u64 = 5000;
const WATCH_TIMEOUT_MS: u64 = 1000;
const REQUIRED_CONFIRMATIONS: u64 = 1;

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
pub(crate) async fn send_transaction_with_retries<P, T, N, E>(
    provider: &P,
    calldata: Calldata,
    to: Address,
    tx_identifier: &str,
    transaction_config: &TransactionConfig,
) -> Result<TxHash, E>
where
    T: Transport + Clone,
    P: Provider<T, N>,
    N: Network,
    E: From<alloy::transports::RpcError<alloy::transports::TransportErrorKind>>
        + From<alloy::providers::PendingTransactionError>
        + From<tokio::time::error::Elapsed>,
{
    let mut nonce: Option<u64> = None;
    let mut retries = 0;

    loop {
        let mut transaction_request = provider
            .transaction_request()
            .with_to(to)
            .with_input(calldata.clone())
            .with_max_fee_per_gas(transaction_config.max_fee_per_gas);

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
            Ok(Err(err)) => {
                if retries == MAX_RETRIES {
                    error!("Failed to send {tx_identifier} transaction after {retries} retries. Giving up. Error: {err:?}");
                    break Err(E::from(err));
                }

                retries += 1;
                let retry_interval_ms = DEFAULT_RETRY_INTERVAL_MS;
                let delay = Duration::from_millis(retry_interval_ms * retries.pow(2) as u64);

                warn!(
                        "Error sending {tx_identifier} transaction: {err:?}. Retry #{} in {} second(s).",
                        retries,
                        delay.as_secs(),
                    );

                tokio::time::sleep(delay).await;

                continue;
            }
            Err(err) => {
                if retries == MAX_RETRIES {
                    error!("Failed to send {tx_identifier} transaction after {retries} retries. Giving up. Error: {err:?}");
                    break Err(E::from(err));
                }

                retries += 1;
                let retry_interval_ms = DEFAULT_RETRY_INTERVAL_MS;
                let delay = Duration::from_millis(retry_interval_ms * retries.pow(2) as u64);

                warn!(
                        "Error sending {tx_identifier} transaction: {err:?}. Retry #{} in {} second(s).",
                        retries,
                        delay.as_secs(),
                    );

                tokio::time::sleep(delay).await;

                continue;
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
                .with_required_confirmations(REQUIRED_CONFIRMATIONS)
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
                break Ok(tx_hash);
            }
            Err(err) => {
                if retries == MAX_RETRIES {
                    error!("Failed to confirm {tx_identifier} transaction after {retries} retries. Giving up. Error: {err:?}");
                    break Err(E::from(err));
                }

                retries += 1;
                let retry_interval_ms = DEFAULT_RETRY_INTERVAL_MS;
                let delay = Duration::from_millis(retry_interval_ms * retries.pow(2) as u64);

                warn!(
                    "Error confirming {tx_identifier} transaction: {err:?}. Retry #{} in {} second(s).",
                    retries,
                    delay.as_secs(),
                );

                tokio::time::sleep(delay).await;
            }
        }
    }
}
