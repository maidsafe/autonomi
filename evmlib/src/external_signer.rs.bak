// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::common::{Address, Amount, Calldata, QuoteHash, QuotePayment, U256};
use crate::contract::network_token::{self, NetworkToken};
use crate::contract::payment_vault::MAX_TRANSFERS_PER_TRANSACTION;
use crate::utils::http_provider;
use crate::Network;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Network token contract error: {0}")]
    NetworkTokenContract(#[from] network_token::Error),
    #[error("Data payments contract error: {0}")]
    DataPaymentsContract(#[from] crate::contract::payment_vault::error::Error),
}

/// Approve an address / smart contract to spend this wallet's payment tokens.
///
/// Returns the transaction calldata (input, to).
pub fn approve_to_spend_tokens_calldata(
    network: &Network,
    spender: Address,
    value: U256,
) -> (Calldata, Address) {
    let provider = http_provider(network.rpc_url().clone());
    let network_token = NetworkToken::new(*network.payment_token_address(), provider);
    network_token.approve_calldata(spender, value)
}

/// Transfer payment tokens from the supplied wallet to an address.
///
/// Returns the transaction calldata (input, to).
pub fn transfer_tokens_calldata(
    network: &Network,
    receiver: Address,
    amount: U256,
) -> (Calldata, Address) {
    let provider = http_provider(network.rpc_url().clone());
    let network_token = NetworkToken::new(*network.payment_token_address(), provider);
    network_token.transfer_calldata(receiver, amount)
}

#[derive(Serialize, Deserialize)]
pub struct PayForQuotesCalldataReturnType {
    pub batched_calldata_map: HashMap<Calldata, Vec<QuoteHash>>,
    pub to: Address,
    pub approve_spender: Address,
    pub approve_amount: Amount,
}

/// Use this wallet to pay for chunks in batched transfer transactions.
/// If the amount of transfers is more than one transaction can contain, the transfers will be split up over multiple transactions.
///
/// Returns PayForQuotesCalldataReturnType, containing calldata of the transaction batches along with the approval details for the spender.
pub fn pay_for_quotes_calldata<T: IntoIterator<Item = QuotePayment>>(
    network: &Network,
    payments: T,
) -> Result<PayForQuotesCalldataReturnType, Error> {
    let payments: Vec<_> = payments.into_iter().collect();

    let total_amount = payments.iter().map(|(_, _, amount)| amount).sum();

    let approve_spender = *network.data_payments_address();
    let approve_amount = total_amount;

    let provider = http_provider(network.rpc_url().clone());
    let data_payments = crate::contract::payment_vault::handler::PaymentVaultHandler::new(
        *network.data_payments_address(),
        provider,
    );

    // Divide transfers over multiple transactions if they exceed the max per transaction.
    let chunks = payments.chunks(MAX_TRANSFERS_PER_TRANSACTION);

    let mut calldata_map: HashMap<Calldata, Vec<QuoteHash>> = HashMap::new();

    for batch in chunks {
        let quote_payments = batch.to_vec();
        let (calldata, _) = data_payments.pay_for_quotes_calldata(quote_payments.clone())?;
        let quote_hashes = quote_payments.into_iter().map(|(qh, _, _)| qh).collect();
        calldata_map.insert(calldata, quote_hashes);
    }

    Ok(PayForQuotesCalldataReturnType {
        batched_calldata_map: calldata_map,
        to: *data_payments.contract.address(),
        approve_spender,
        approve_amount,
    })
}
