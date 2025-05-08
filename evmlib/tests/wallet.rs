mod common;

use crate::common::quote::random_quote_payment;
use alloy::network::EthereumWallet;
use alloy::node_bindings::AnvilInstance;
use alloy::primitives::utils::parse_ether;
use alloy::providers::ext::AnvilApi;
use alloy::providers::{ProviderBuilder, WalletProvider};
use alloy::signers::local::{LocalSigner, PrivateKeySigner};
use evmlib::common::{Amount, TxHash};
use evmlib::contract::payment_vault::{verify_data_payment, MAX_TRANSFERS_PER_TRANSACTION};
use evmlib::quoting_metrics::QuotingMetrics;
use evmlib::testnet::{deploy_data_payments_contract, deploy_network_token_contract, start_node};
use evmlib::transaction_config::TransactionConfig;
use evmlib::wallet::{transfer_tokens, wallet_address, Wallet};
use evmlib::{CustomNetwork, Network};
use std::collections::HashSet;

#[allow(clippy::unwrap_used)]
async fn local_testnet() -> (AnvilInstance, Network, EthereumWallet) {
    let (node, rpc_url) = start_node();
    let network_token = deploy_network_token_contract(&rpc_url, &node).await;
    let payment_token_address = *network_token.contract.address();
    let data_payments = deploy_data_payments_contract(&rpc_url, &node, payment_token_address).await;

    (
        node,
        Network::Custom(CustomNetwork {
            rpc_url_http: rpc_url,
            payment_token_address,
            data_payments_address: *data_payments.contract.address(),
        }),
        network_token.contract.provider().wallet().clone(),
    )
}

#[allow(clippy::unwrap_used)]
async fn funded_wallet(network: &Network, genesis_wallet: EthereumWallet) -> Wallet {
    let signer: PrivateKeySigner = LocalSigner::random();
    let wallet = EthereumWallet::from(signer);
    let account = wallet_address(&wallet);

    let provider = ProviderBuilder::new()
        .with_simple_nonce_management()
        .wallet(genesis_wallet.clone())
        .connect_http(network.rpc_url().clone());

    // Fund the wallet with plenty of gas tokens
    provider
        .anvil_set_balance(account, parse_ether("1000").expect(""))
        .await
        .unwrap();

    let transaction_config = TransactionConfig::default();

    // Fund the wallet with plenty of ERC20 tokens
    transfer_tokens(
        genesis_wallet,
        network,
        account,
        Amount::from(9999999999_u64),
        &transaction_config,
    )
    .await
    .unwrap();

    Wallet::new(network.clone(), wallet)
}

#[tokio::test]
async fn test_pay_for_quotes_and_data_payment_verification() {
    const TRANSFERS: usize = 600;

    let (_anvil, network, genesis_wallet) = local_testnet().await;
    let wallet = funded_wallet(&network, genesis_wallet).await;

    let mut quote_payments = vec![];

    for _ in 0..TRANSFERS {
        let quote = random_quote_payment();
        quote_payments.push(quote);
    }

    let tx_hashes = wallet.pay_for_quotes(quote_payments.clone()).await.unwrap();

    let unique_tx_hashes: HashSet<TxHash> = tx_hashes.values().cloned().collect();

    assert_eq!(
        unique_tx_hashes.len(),
        TRANSFERS.div_ceil(MAX_TRANSFERS_PER_TRANSACTION)
    );
    for (quote_hash, reward_addr, _) in quote_payments.iter() {
        let result = verify_data_payment(
            &network,
            vec![*quote_hash],
            vec![(
                *quote_hash,
                QuotingMetrics {
                    data_size: 0,
                    data_type: 0,
                    close_records_stored: 0,
                    records_per_type: vec![],
                    max_records: 0,
                    received_payment_count: 0,
                    live_time: 0,
                    network_density: None,
                    network_size: None,
                },
                *reward_addr,
            )],
        )
        .await;

        assert!(
            result.is_ok(),
            "Verification failed for: {quote_hash:?}. Error: {:?}",
            result.err()
        );
    }
}
