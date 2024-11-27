// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::common::{Address, QuoteHash, TxHash, U256};
use crate::event::{ChunkPaymentEvent, DATA_PAYMENT_EVENT_SIGNATURE};
use crate::Network;
use alloy::eips::BlockNumberOrTag;
use alloy::primitives::FixedBytes;
use alloy::providers::{Provider, ProviderBuilder};
use alloy::rpc::types::{Block, Filter, Log, TransactionReceipt};
use alloy::transports::{RpcError, TransportErrorKind};

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error(transparent)]
    RpcError(#[from] RpcError<TransportErrorKind>),
    #[error("Transaction is not confirmed")]
    TransactionUnconfirmed,
    #[error("Transaction was not found")]
    TransactionNotFound,
    #[error("Transaction has not been included in a block yet")]
    TransactionNotInBlock,
    #[error("Block was not found")]
    BlockNotFound,
    #[error("No event proof found")]
    EventProofNotFound,
    #[error("Payment was done after the quote expired")]
    QuoteExpired,
}

/// Get a transaction receipt by its hash.
pub async fn get_transaction_receipt_by_hash(
    network: &Network,
    transaction_hash: TxHash,
) -> Result<Option<TransactionReceipt>, Error> {
    let provider = ProviderBuilder::new()
        .with_recommended_fillers()
        .on_http(network.rpc_url().clone());
    let maybe_receipt = provider
        .get_transaction_receipt(transaction_hash)
        .await
        .inspect_err(|err| error!("Error getting transaction receipt for transaction_hash: {transaction_hash:?} : {err:?}", ))?;
    debug!("Transaction receipt for {transaction_hash:?} : {maybe_receipt:?}",);
    Ok(maybe_receipt)
}

/// Get a block by its block number.
async fn get_block_by_number(network: &Network, block_number: u64) -> Result<Option<Block>, Error> {
    let provider = ProviderBuilder::new()
        .with_recommended_fillers()
        .on_http(network.rpc_url().clone());
    let block = provider
        .get_block_by_number(BlockNumberOrTag::Number(block_number), true)
        .await
        .inspect_err(|err| error!("Error getting block by number for {block_number} : {err:?}",))?;
    debug!("Block for {block_number} : {block:?}",);
    Ok(block)
}

/// Get transaction logs using a filter.
async fn get_transaction_logs(network: &Network, filter: Filter) -> Result<Vec<Log>, Error> {
    let provider = ProviderBuilder::new()
        .with_recommended_fillers()
        .on_http(network.rpc_url().clone());
    let logs = provider
        .get_logs(&filter)
        .await
        .inspect_err(|err| error!("Error getting logs for filter: {filter:?} : {err:?}"))?;
    debug!("Transaction logs is fetched");
    Ok(logs)
}

/// Get a DataPaymentMade event, filtered by a hashed chunk address and a node address.
/// Useful for a node if it wants to check if payment for a certain chunk has been made.
async fn get_data_payment_event(
    network: &Network,
    block_number: u64,
    quote_hash: QuoteHash,
    reward_addr: Address,
    amount: U256,
) -> Result<Vec<Log>, Error> {
    debug!(
        "Getting data payment event for quote_hash: {quote_hash:?}, reward_addr: {reward_addr:?}"
    );
    let topic1: FixedBytes<32> = FixedBytes::left_padding_from(reward_addr.as_slice());

    let filter = Filter::new()
        .event_signature(DATA_PAYMENT_EVENT_SIGNATURE)
        .topic1(topic1)
        .topic2(amount)
        .topic3(quote_hash)
        .from_block(block_number)
        .to_block(block_number);

    get_transaction_logs(network, filter).await
}

/// Verify if a data payment is confirmed.
pub async fn verify_data_payment(
    network: &Network,
    tx_hash: TxHash,
    quote_hash: QuoteHash,
    reward_addr: Address,
    amount: U256,
    quote_expiration_timestamp_in_secs: u64,
) -> Result<(), Error> {
    debug!("Verifying data payment for tx_hash: {tx_hash:?}");
    let transaction = get_transaction_receipt_by_hash(network, tx_hash)
        .await?
        .ok_or(Error::TransactionNotFound)?;

    // If the status is True, it means the tx is confirmed.
    if !transaction.status() {
        error!("Transaction {tx_hash:?} is not confirmed");
        return Err(Error::TransactionUnconfirmed);
    }

    let block_number = transaction
        .block_number
        .ok_or(Error::TransactionNotInBlock)
        .inspect_err(|_| error!("Transaction {tx_hash:?} has not been included in a block yet"))?;

    let block = get_block_by_number(network, block_number)
        .await?
        .ok_or(Error::BlockNotFound)?;

    // Check if payment was done within the quote expiration timeframe.
    if quote_expiration_timestamp_in_secs < block.header.timestamp {
        error!("Payment for tx_hash: {tx_hash:?} was done after the quote expired");
        return Err(Error::QuoteExpired);
    }

    let logs =
        get_data_payment_event(network, block_number, quote_hash, reward_addr, amount).await?;

    for log in logs {
        if log.transaction_hash != Some(tx_hash) {
            // Wrong transaction.
            continue;
        }

        if let Ok(event) = ChunkPaymentEvent::try_from(log) {
            // Check if the event matches what we expect.
            if event.quote_hash == quote_hash
                && event.rewards_address == reward_addr
                && event.amount >= amount
            {
                return Ok(());
            }
        }
    }

    error!("No event proof found for tx_hash: {tx_hash:?}");

    Err(Error::EventProofNotFound)
}

#[cfg(test)]
mod tests {
    use crate::common::{Address, U256};
    use crate::transaction::{
        get_data_payment_event, get_transaction_receipt_by_hash, verify_data_payment,
    };
    use crate::Network;
    use alloy::hex::FromHex;
    use alloy::primitives::b256;

    #[tokio::test]
    async fn test_get_transaction_receipt_by_hash() {
        let network = Network::ArbitrumOne;

        let tx_hash = b256!("3304465f38fa0bd9670a426108dd1ddd193e059dcb7c13982d31424646217a36"); // DevSkim: ignore DS173237

        assert!(get_transaction_receipt_by_hash(&network, tx_hash)
            .await
            .unwrap()
            .is_some());
    }

    #[tokio::test]
    async fn test_get_data_payment_event() {
        let network = Network::ArbitrumOne;

        let block_number: u64 = 260246302;
        let reward_address = Address::from_hex("8AB15A43305854e4AE4E6FBEa0CD1CC0AB4ecB2A").unwrap(); // DevSkim: ignore DS173237
        let amount = U256::from(1);
        let quote_hash = b256!("EBD943C38C0422901D4CF22E677DD95F2591CA8D6EBFEA8BAF1BFE9FF5506ECE"); // DevSkim: ignore DS173237

        let logs =
            get_data_payment_event(&network, block_number, quote_hash, reward_address, amount)
                .await
                .unwrap();

        assert_eq!(logs.len(), 1);
    }

    #[tokio::test]
    async fn test_verify_data_payment() {
        let network = Network::ArbitrumOne;

        let tx_hash = b256!("3304465f38fa0bd9670a426108dd1ddd193e059dcb7c13982d31424646217a36"); // DevSkim: ignore DS173237
        let quote_hash = b256!("EBD943C38C0422901D4CF22E677DD95F2591CA8D6EBFEA8BAF1BFE9FF5506ECE"); // DevSkim: ignore DS173237
        let reward_address = Address::from_hex("8AB15A43305854e4AE4E6FBEa0CD1CC0AB4ecB2A").unwrap(); // DevSkim: ignore DS173237
        let amount = U256::from(1);

        let result = verify_data_payment(
            &network,
            tx_hash,
            quote_hash,
            reward_address,
            amount,
            4102441200,
        )
        .await;

        assert!(result.is_ok(), "Error: {:?}", result.err());
    }
}
