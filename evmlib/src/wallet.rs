// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::common::{Address, Amount, QuoteHash, QuotePayment, TxHash, U256};
use crate::contract::network_token::NetworkToken;
use crate::contract::payment_vault::MAX_TRANSFERS_PER_TRANSACTION;
use crate::contract::payment_vault::handler::PaymentVaultHandler;
use crate::contract::{network_token, payment_vault};
use crate::transaction_config::TransactionConfig;
use crate::utils::http_provider;
use crate::{Network, TX_TIMEOUT};
use alloy::hex::ToHexExt;
use alloy::network::{Ethereum, EthereumWallet, NetworkWallet, TransactionBuilder};
use alloy::providers::fillers::{
    BlobGasFiller, ChainIdFiller, FillProvider, GasFiller, JoinFill, NonceFiller,
    SimpleNonceManager, WalletFiller,
};
use alloy::providers::{Identity, Provider, ProviderBuilder, RootProvider};
use alloy::rpc::types::TransactionRequest;
use alloy::signers::local::{LocalSigner, PrivateKeySigner};
use alloy::transports::http::reqwest;
use alloy::transports::{RpcError, TransportErrorKind};
use std::collections::BTreeMap;
use std::sync::Arc;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Insufficient tokens to pay for quotes. Have: {0} atto, need: {1} atto")]
    InsufficientTokensForQuotes(Amount, Amount),
    #[error("Private key is invalid")]
    PrivateKeyInvalid,
    #[error(transparent)]
    RpcError(#[from] RpcError<TransportErrorKind>),
    #[error("Network token contract error: {0}")]
    NetworkTokenContract(#[from] network_token::Error),
    #[error("Chunk payments contract error: {0}")]
    ChunkPaymentsContract(#[from] payment_vault::error::Error),
}

#[derive(Clone, Debug)]
pub struct Wallet {
    wallet: EthereumWallet,
    network: Network,
    transaction_config: TransactionConfig,
    lock: Arc<tokio::sync::Mutex<()>>,
}

impl Wallet {
    /// Creates a new Wallet object with the specific Network and EthereumWallet.
    pub fn new(network: Network, wallet: EthereumWallet) -> Self {
        Self {
            wallet,
            network,
            transaction_config: Default::default(),
            lock: Arc::new(tokio::sync::Mutex::new(())),
        }
    }

    /// Convenience function that creates a new Wallet with a random EthereumWallet.
    pub fn new_with_random_wallet(network: Network) -> Self {
        Self::new(network, random())
    }

    /// Creates a new Wallet based on the given Ethereum private key. It will fail with Error::PrivateKeyInvalid if private_key is invalid.
    pub fn new_from_private_key(network: Network, private_key: &str) -> Result<Self, Error> {
        let wallet = from_private_key(private_key)?;
        Ok(Self::new(network, wallet))
    }

    /// Returns the address of this wallet.
    pub fn address(&self) -> Address {
        wallet_address(&self.wallet)
    }

    /// Returns the `Network` of this wallet.
    pub fn network(&self) -> &Network {
        &self.network
    }

    /// Returns the raw balance of payment tokens for this wallet.
    pub async fn balance_of_tokens(&self) -> Result<U256, network_token::Error> {
        balance_of_tokens(self.address(), &self.network).await
    }

    /// Returns the raw balance of gas tokens for this wallet.
    pub async fn balance_of_gas_tokens(&self) -> Result<U256, network_token::Error> {
        balance_of_gas_tokens(self.address(), &self.network).await
    }

    /// Transfer a raw amount of payment tokens to another address.
    pub async fn transfer_tokens(
        &self,
        to: Address,
        amount: U256,
    ) -> Result<TxHash, network_token::Error> {
        transfer_tokens(
            self.wallet.clone(),
            &self.network,
            to,
            amount,
            &self.transaction_config,
        )
        .await
    }

    /// Transfer a raw amount of gas tokens to another address.
    pub async fn transfer_gas_tokens(
        &self,
        to: Address,
        amount: U256,
    ) -> Result<TxHash, network_token::Error> {
        transfer_gas_tokens(self.wallet.clone(), &self.network, to, amount).await
    }

    /// See how many tokens of the owner may be spent by the spender.
    pub async fn token_allowance(&self, spender: Address) -> Result<U256, network_token::Error> {
        token_allowance(&self.network, self.address(), spender).await
    }

    /// Approve an address / smart contract to spend this wallet's payment tokens.
    pub async fn approve_to_spend_tokens(
        &self,
        spender: Address,
        amount: U256,
    ) -> Result<TxHash, network_token::Error> {
        approve_to_spend_tokens(
            self.wallet.clone(),
            &self.network,
            spender,
            amount,
            &self.transaction_config,
        )
        .await
    }

    /// Function for batch payments of quotes. It accepts an iterator of QuotePayment and returns
    /// transaction hashes of the payments by quotes.
    pub async fn pay_for_quotes<I: IntoIterator<Item = QuotePayment>>(
        &self,
        quote_payments: I,
    ) -> Result<BTreeMap<QuoteHash, TxHash>, PayForQuotesError> {
        pay_for_quotes(
            self.wallet.clone(),
            &self.network,
            quote_payments,
            &self.transaction_config,
        )
        .await
    }

    /// Build a provider using this wallet.
    pub fn to_provider(&self) -> ProviderWithWallet {
        http_provider_with_wallet(self.network.rpc_url().clone(), self.wallet.clone())
    }

    /// Lock the wallet to prevent concurrent use.
    /// Drop the guard to unlock the wallet.
    pub async fn lock(&self) -> tokio::sync::MutexGuard<()> {
        self.lock.lock().await
    }

    /// Returns a random private key string.
    pub fn random_private_key() -> String {
        let signer: PrivateKeySigner = LocalSigner::random();
        signer.to_bytes().encode_hex_with_prefix()
    }

    /// Sets the transaction configuration for the wallet.
    pub fn set_transaction_config(&mut self, config: TransactionConfig) {
        self.transaction_config = config;
    }
}

/// Generate an EthereumWallet with a random private key.
fn random() -> EthereumWallet {
    let signer: PrivateKeySigner = LocalSigner::random();
    EthereumWallet::from(signer)
}

/// Creates a wallet from a private key in HEX format.
fn from_private_key(private_key: &str) -> Result<EthereumWallet, Error> {
    let signer: PrivateKeySigner = private_key.parse().map_err(|err| {
        error!("Error parsing private key: {err}");
        Error::PrivateKeyInvalid
    })?;
    Ok(EthereumWallet::from(signer))
}

// TODO(optimization): Find a way to reuse/persist contracts and/or a provider without the wallet nonce going out of sync

pub type ProviderWithWallet = FillProvider<
    JoinFill<
        JoinFill<
            JoinFill<
                Identity,
                JoinFill<GasFiller, JoinFill<BlobGasFiller, JoinFill<NonceFiller, ChainIdFiller>>>,
            >,
            NonceFiller<SimpleNonceManager>,
        >,
        WalletFiller<EthereumWallet>,
    >,
    RootProvider,
    Ethereum,
>;

fn http_provider_with_wallet(rpc_url: reqwest::Url, wallet: EthereumWallet) -> ProviderWithWallet {
    ProviderBuilder::new()
        .with_simple_nonce_management()
        .wallet(wallet)
        .connect_http(rpc_url)
}

/// Returns the address of this wallet.
pub fn wallet_address(wallet: &EthereumWallet) -> Address {
    <EthereumWallet as NetworkWallet<Ethereum>>::default_signer_address(wallet)
}

/// Returns the raw balance of payment tokens for this wallet.
pub async fn balance_of_tokens(
    account: Address,
    network: &Network,
) -> Result<U256, network_token::Error> {
    info!("Getting balance of tokens for account: {account}");
    let provider = http_provider(network.rpc_url().clone());
    let network_token = NetworkToken::new(*network.payment_token_address(), provider);
    network_token.balance_of(account).await
}

/// Returns the raw balance of gas tokens for this wallet.
pub async fn balance_of_gas_tokens(
    account: Address,
    network: &Network,
) -> Result<U256, network_token::Error> {
    debug!("Getting balance of gas tokens for account: {account}");
    let provider = http_provider(network.rpc_url().clone());
    let balance = provider.get_balance(account).await?;
    Ok(balance)
}

/// See how many tokens of the owner may be spent by the spender.
pub async fn token_allowance(
    network: &Network,
    owner: Address,
    spender: Address,
) -> Result<U256, network_token::Error> {
    debug!("Getting allowance for owner: {owner} and spender: {spender}",);
    let provider = http_provider(network.rpc_url().clone());
    let network_token = NetworkToken::new(*network.payment_token_address(), provider);
    network_token.allowance(owner, spender).await
}

/// Approve an address / smart contract to spend this wallet's payment tokens.
pub async fn approve_to_spend_tokens(
    wallet: EthereumWallet,
    network: &Network,
    spender: Address,
    amount: U256,
    transaction_config: &TransactionConfig,
) -> Result<TxHash, network_token::Error> {
    debug!("Approving address/smart contract with {amount} tokens at address: {spender}",);
    let provider = http_provider_with_wallet(network.rpc_url().clone(), wallet);
    let network_token = NetworkToken::new(*network.payment_token_address(), provider);
    network_token
        .approve(spender, amount, transaction_config)
        .await
}

/// Transfer payment tokens from the supplied wallet to an address.
pub async fn transfer_tokens(
    wallet: EthereumWallet,
    network: &Network,
    receiver: Address,
    amount: U256,
    transaction_config: &TransactionConfig,
) -> Result<TxHash, network_token::Error> {
    debug!("Transferring {amount} tokens to {receiver}");
    let provider = http_provider_with_wallet(network.rpc_url().clone(), wallet);
    let network_token = NetworkToken::new(*network.payment_token_address(), provider);
    network_token
        .transfer(receiver, amount, transaction_config)
        .await
}

/// Transfer native/gas tokens from the supplied wallet to an address.
pub async fn transfer_gas_tokens(
    wallet: EthereumWallet,
    network: &Network,
    receiver: Address,
    amount: U256,
) -> Result<TxHash, network_token::Error> {
    debug!("Transferring {amount} gas tokens to {receiver}");
    let provider = http_provider_with_wallet(network.rpc_url().clone(), wallet);
    let tx = TransactionRequest::default()
        .with_to(receiver)
        .with_value(amount);

    let pending_tx_builder = provider
        .send_transaction(tx)
        .await
        .inspect_err(|err| {
            error!("Error to send_transaction during transfer_gas_tokens: {err}");
        })?
        .with_timeout(Some(TX_TIMEOUT));
    let pending_tx_hash = *pending_tx_builder.tx_hash();
    debug!("The transfer of gas tokens is pending with tx_hash: {pending_tx_hash}");

    let tx_hash = pending_tx_builder.watch().await.inspect_err(|err| {
        error!("Error watching transfer_gas_tokens tx with hash {pending_tx_hash}: {err}")
    })?;
    debug!("Transfer of gas tokens with tx_hash: {tx_hash} is successful");

    Ok(tx_hash)
}

/// Contains the payment error and the already succeeded batch payments (if any).
#[derive(Debug)]
pub struct PayForQuotesError(pub Error, pub BTreeMap<QuoteHash, TxHash>);

/// Use this wallet to pay for chunks in batched transfer transactions.
/// If the amount of transfers is more than one transaction can contain, the transfers will be split up over multiple transactions.
pub async fn pay_for_quotes<T: IntoIterator<Item = QuotePayment>>(
    wallet: EthereumWallet,
    network: &Network,
    payments: T,
    transaction_config: &TransactionConfig,
) -> Result<BTreeMap<QuoteHash, TxHash>, PayForQuotesError> {
    let payments: Vec<_> = payments.into_iter().collect();
    info!("Paying for quotes of len: {}", payments.len());

    let total_amount_to_be_paid = payments.iter().map(|(_, _, amount)| amount).sum();

    // Get current wallet token balance
    let wallet_balance = balance_of_tokens(wallet_address(&wallet), network)
        .await
        .map_err(|err| PayForQuotesError(Error::from(err), Default::default()))?;

    // Check if wallet contains enough payment tokens to pay for all quotes
    if wallet_balance < total_amount_to_be_paid {
        return Err(PayForQuotesError(
            Error::InsufficientTokensForQuotes(wallet_balance, total_amount_to_be_paid),
            Default::default(),
        ));
    }

    // Get current allowance
    let allowance = token_allowance(
        network,
        wallet_address(&wallet),
        *network.data_payments_address(),
    )
    .await
    .map_err(|err| PayForQuotesError(Error::from(err), Default::default()))?;

    // TODO: Get rid of approvals altogether, by using permits or whatever..
    if allowance < total_amount_to_be_paid {
        // Approve the contract to spend all the client's tokens.
        approve_to_spend_tokens(
            wallet.clone(),
            network,
            *network.data_payments_address(),
            U256::MAX,
            transaction_config,
        )
        .await
        .map_err(|err| PayForQuotesError(Error::from(err), Default::default()))?;
    }

    let provider = http_provider_with_wallet(network.rpc_url().clone(), wallet);
    let data_payments = PaymentVaultHandler::new(*network.data_payments_address(), provider);

    // remove payments with 0 amount as they don't need to be paid for
    let payment_for_batch: Vec<QuotePayment> = payments
        .into_iter()
        .filter(|(_, _, amount)| *amount > Amount::ZERO)
        .collect();

    // Divide transfers over multiple transactions if they exceed the max per transaction.
    let chunks = payment_for_batch.chunks(MAX_TRANSFERS_PER_TRANSACTION);

    let mut tx_hashes_by_quote = BTreeMap::new();

    for batch in chunks {
        let batch: Vec<QuotePayment> = batch.to_vec();

        debug!(
            "Paying for batch of quotes of len: {}, {batch:?}",
            batch.len()
        );

        let tx_hash = data_payments
            .pay_for_quotes(batch.clone(), transaction_config)
            .await
            .map_err(|err| PayForQuotesError(Error::from(err), tx_hashes_by_quote.clone()))?;

        info!("Paid for batch of quotes with final tx hash: {tx_hash}");

        for (quote_hash, _, _) in batch {
            tx_hashes_by_quote.insert(quote_hash, tx_hash);
        }
    }

    Ok(tx_hashes_by_quote)
}

#[cfg(test)]
mod tests {
    use crate::common::Amount;
    use crate::testnet::Testnet;
    use crate::wallet::{Wallet, from_private_key};
    use alloy::network::{Ethereum, EthereumWallet, NetworkWallet};
    use alloy::primitives::address;

    #[tokio::test]
    async fn test_from_private_key() {
        let private_key = "bf210844fa5463e373974f3d6fbedf451350c3e72b81b3c5b1718cb91f49c33d"; // DevSkim: ignore DS117838
        let wallet = from_private_key(private_key).unwrap();
        let account = <EthereumWallet as NetworkWallet<Ethereum>>::default_signer_address(&wallet);

        // Assert that the addresses are the same, i.e. the wallet was successfully created from the private key
        assert_eq!(
            account,
            address!("1975d01f46D70AAc0dd3fCf942d92650eE63C79A")
        );
    }

    #[tokio::test]
    async fn test_transfer_gas_tokens() {
        let testnet = Testnet::new().await;
        let network = testnet.to_network();
        let wallet =
            Wallet::new_from_private_key(network.clone(), &testnet.default_wallet_private_key())
                .unwrap();
        let receiver_wallet = Wallet::new_with_random_wallet(network);
        let transfer_amount = Amount::from(117);

        let initial_balance = receiver_wallet.balance_of_gas_tokens().await.unwrap();

        assert_eq!(initial_balance, Amount::from(0));

        let _ = wallet
            .transfer_gas_tokens(receiver_wallet.address(), transfer_amount)
            .await
            .unwrap();

        let final_balance = receiver_wallet.balance_of_gas_tokens().await.unwrap();

        assert_eq!(final_balance, transfer_amount);
    }
}
