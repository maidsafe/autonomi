#![allow(clippy::expect_used)]

mod common;

use crate::common::quote::random_quote_payment;
use alloy::network::{Ethereum, EthereumWallet};
use alloy::node_bindings::AnvilInstance;
use alloy::primitives::utils::parse_ether;
use alloy::providers::ext::AnvilApi;
use alloy::providers::fillers::{
    BlobGasFiller, ChainIdFiller, FillProvider, GasFiller, JoinFill, NonceFiller,
    SimpleNonceManager, WalletFiller,
};
use alloy::providers::{Identity, ProviderBuilder, RootProvider, WalletProvider};
use alloy::signers::local::{LocalSigner, PrivateKeySigner};
use evmlib::Network;
use evmlib::common::U256;
use evmlib::contract::network_token::NetworkToken;
use evmlib::contract::payment_vault::handler::PaymentVaultHandler;
use evmlib::contract::payment_vault::{MAX_TRANSFERS_PER_TRANSACTION, interface};
use evmlib::quoting_metrics::QuotingMetrics;
use evmlib::testnet::{deploy_data_payments_contract, deploy_network_token_contract, start_node};
use evmlib::transaction_config::TransactionConfig;
use evmlib::utils::http_provider;
use evmlib::wallet::wallet_address;

async fn setup() -> (
    AnvilInstance,
    NetworkToken<
        FillProvider<
            JoinFill<
                JoinFill<
                    JoinFill<
                        Identity,
                        JoinFill<
                            GasFiller,
                            JoinFill<BlobGasFiller, JoinFill<NonceFiller, ChainIdFiller>>,
                        >,
                    >,
                    NonceFiller<SimpleNonceManager>,
                >,
                WalletFiller<EthereumWallet>,
            >,
            RootProvider,
            Ethereum,
        >,
        Ethereum,
    >,
    PaymentVaultHandler<
        FillProvider<
            JoinFill<
                JoinFill<
                    JoinFill<
                        Identity,
                        JoinFill<
                            GasFiller,
                            JoinFill<BlobGasFiller, JoinFill<NonceFiller, ChainIdFiller>>,
                        >,
                    >,
                    NonceFiller<SimpleNonceManager>,
                >,
                WalletFiller<EthereumWallet>,
            >,
            RootProvider,
            Ethereum,
        >,
        Ethereum,
    >,
) {
    let (node, rpc_url) = start_node();

    let network_token = deploy_network_token_contract(&rpc_url, &node).await;

    let data_payments =
        deploy_data_payments_contract(&rpc_url, &node, *network_token.contract.address()).await;

    (node, network_token, data_payments)
}

#[allow(clippy::unwrap_used)]
#[allow(clippy::type_complexity)]
#[allow(dead_code)]
async fn provider_with_gas_funded_wallet(
    anvil: &AnvilInstance,
) -> FillProvider<
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
> {
    let signer: PrivateKeySigner = LocalSigner::random();
    let wallet = EthereumWallet::from(signer);

    let rpc_url = anvil.endpoint().parse().unwrap();

    let provider = ProviderBuilder::new()
        .with_simple_nonce_management()
        .wallet(wallet)
        .connect_http(rpc_url);

    let account = wallet_address(provider.wallet());

    // Fund the wallet with plenty of gas tokens
    provider
        .anvil_set_balance(account, parse_ether("1000").expect(""))
        .await
        .unwrap();

    provider
}

#[tokio::test]
async fn test_deploy() {
    setup().await;
}

#[tokio::test]
async fn test_get_quote_on_arb_sepolia() {
    let network = Network::ArbitrumSepoliaTest;
    let provider = http_provider(network.rpc_url().clone());
    let payment_vault = PaymentVaultHandler::new(*network.data_payments_address(), provider);

    let quoting_metrics = QuotingMetrics {
        data_type: 1, // a GraphEntry record
        data_size: 100,
        close_records_stored: 10,
        records_per_type: vec![(0, 5), (1, 5)],
        max_records: 16 * 1024,
        received_payment_count: 0,
        live_time: 1400,
        network_density: Some([
            4, 4, 224, 228, 247, 252, 14, 44, 67, 21, 153, 47, 244, 18, 232, 1, 152, 195, 44, 43,
            29, 135, 19, 217, 240, 129, 64, 245, 240, 227, 129, 162,
        ]),
        network_size: Some(240),
    };

    let result = payment_vault.get_quote(vec![quoting_metrics]).await;

    assert!(result.is_ok(), "Failed with error: {:?}", result.err());
}

#[tokio::test]
async fn test_pay_for_quotes_on_local() {
    let (_anvil, network_token, mut payment_vault) = setup().await;

    let transaction_config = TransactionConfig::default();

    let mut quote_payments = vec![];

    for _ in 0..MAX_TRANSFERS_PER_TRANSACTION {
        let quote_payment = random_quote_payment();
        quote_payments.push(quote_payment);
    }

    let _ = network_token
        .approve(
            *payment_vault.contract.address(),
            U256::MAX,
            &transaction_config,
        )
        .await
        .unwrap();

    // Contract provider has a different account coupled to it,
    // so we set it to the same as the network token contract
    payment_vault.set_provider(network_token.contract.provider().clone());

    let result = payment_vault
        .pay_for_quotes(quote_payments, &transaction_config)
        .await;

    assert!(result.is_ok(), "Failed with error: {:?}", result.err());
}

#[tokio::test]
async fn test_verify_payment_on_local() {
    let (_anvil, network_token, mut payment_vault) = setup().await;

    let transaction_config = TransactionConfig::default();

    let mut quote_payments = vec![];

    for _ in 0..5 {
        let quote_payment = random_quote_payment();
        quote_payments.push(quote_payment);
    }

    let _ = network_token
        .approve(
            *payment_vault.contract.address(),
            U256::MAX,
            &transaction_config,
        )
        .await
        .unwrap();

    // Contract provider has a different account coupled to it,
    // so we set it to the same as the network token contract
    payment_vault.set_provider(network_token.contract.provider().clone());

    let result = payment_vault
        .pay_for_quotes(quote_payments.clone(), &transaction_config)
        .await;

    assert!(result.is_ok(), "Failed with error: {:?}", result.err());

    let payment_verifications: Vec<_> = quote_payments
        .into_iter()
        .map(|v| interface::IPaymentVault::PaymentVerification {
            metrics: QuotingMetrics {
                data_size: 0,
                data_type: 0,
                close_records_stored: 0,
                records_per_type: vec![],
                max_records: 0,
                received_payment_count: 0,
                live_time: 0,
                network_density: None,
                network_size: None,
            }
            .into(),
            rewardsAddress: v.1,
            quoteHash: v.0,
        })
        .collect();

    let results = payment_vault
        .verify_payment(payment_verifications)
        .await
        .expect("Verify payment failed");

    for result in results {
        assert!(result.isValid);
    }
}
