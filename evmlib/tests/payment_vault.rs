mod common;

use crate::common::quote::random_quote_payment;
use alloy::network::{Ethereum, EthereumWallet};
use alloy::node_bindings::AnvilInstance;
use alloy::primitives::utils::parse_ether;
use alloy::providers::ext::AnvilApi;
use alloy::providers::fillers::{
    BlobGasFiller, ChainIdFiller, FillProvider, GasFiller, JoinFill, NonceFiller, WalletFiller,
};
use alloy::providers::{Identity, ProviderBuilder, ReqwestProvider, WalletProvider};
use alloy::signers::local::{LocalSigner, PrivateKeySigner};
use alloy::transports::http::{Client, Http};
use evmlib::common::{Amount, U256};
use evmlib::contract::network_token::NetworkToken;
use evmlib::contract::payment_vault::handler::PaymentVaultHandler;
use evmlib::contract::payment_vault::{interface, MAX_TRANSFERS_PER_TRANSACTION};
use evmlib::quoting_metrics::QuotingMetrics;
use evmlib::testnet::{deploy_data_payments_contract, deploy_network_token_contract, start_node};
use evmlib::utils::http_provider;
use evmlib::wallet::wallet_address;
use evmlib::Network;

async fn setup() -> (
    AnvilInstance,
    NetworkToken<
        Http<Client>,
        FillProvider<
            JoinFill<
                JoinFill<
                    Identity,
                    JoinFill<
                        GasFiller,
                        JoinFill<BlobGasFiller, JoinFill<NonceFiller, ChainIdFiller>>,
                    >,
                >,
                WalletFiller<EthereumWallet>,
            >,
            ReqwestProvider,
            Http<Client>,
            Ethereum,
        >,
        Ethereum,
    >,
    PaymentVaultHandler<
        Http<Client>,
        FillProvider<
            JoinFill<
                JoinFill<
                    Identity,
                    JoinFill<
                        GasFiller,
                        JoinFill<BlobGasFiller, JoinFill<NonceFiller, ChainIdFiller>>,
                    >,
                >,
                WalletFiller<EthereumWallet>,
            >,
            ReqwestProvider,
            Http<Client>,
            Ethereum,
        >,
        Ethereum,
    >,
) {
    let (anvil, rpc_url) = start_node();

    let network_token = deploy_network_token_contract(&rpc_url, &anvil).await;

    let data_payments =
        deploy_data_payments_contract(&rpc_url, &anvil, *network_token.contract.address()).await;

    (anvil, network_token, data_payments)
}

#[allow(clippy::unwrap_used)]
#[allow(clippy::type_complexity)]
#[allow(dead_code)]
async fn provider_with_gas_funded_wallet(
    anvil: &AnvilInstance,
) -> FillProvider<
    JoinFill<
        JoinFill<
            Identity,
            JoinFill<GasFiller, JoinFill<BlobGasFiller, JoinFill<NonceFiller, ChainIdFiller>>>,
        >,
        WalletFiller<EthereumWallet>,
    >,
    ReqwestProvider,
    Http<Client>,
    Ethereum,
> {
    let signer: PrivateKeySigner = LocalSigner::random();
    let wallet = EthereumWallet::from(signer);

    let rpc_url = anvil.endpoint().parse().unwrap();

    let provider = ProviderBuilder::new()
        .with_recommended_fillers()
        .wallet(wallet)
        .on_http(rpc_url);

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
async fn test_proxy_reachable() {
    let network = Network::ArbitrumOne;
    let provider = http_provider(network.rpc_url().clone());
    let payment_vault = PaymentVaultHandler::new(*network.data_payments_address(), provider);

    let amount = payment_vault
        .get_quote(vec![QuotingMetrics {
            close_records_stored: 0,
            max_records: 0,
            received_payment_count: 0,
            live_time: 0,
            network_density: None,
            network_size: None,
        }])
        .await
        .unwrap();

    assert_eq!(amount, vec![Amount::from(1)]);
}

#[tokio::test]
async fn test_verify_payment() {
    let (_anvil, network_token, mut payment_vault) = setup().await;

    let mut quote_payments = vec![];

    for _ in 0..5 {
        let quote_payment = random_quote_payment();
        quote_payments.push(quote_payment);
    }

    let _ = network_token
        .approve(*payment_vault.contract.address(), U256::MAX)
        .await
        .unwrap();

    // Contract provider has a different account coupled to it,
    // so we set it to the same as the network token contract
    payment_vault.set_provider(network_token.contract.provider().clone());

    let result = payment_vault.pay_for_quotes(quote_payments.clone()).await;

    assert!(result.is_ok(), "Failed with error: {:?}", result.err());

    let payment_verifications: Vec<_> = quote_payments
        .into_iter()
        .map(|v| interface::IPaymentVault::PaymentVerification {
            metrics: QuotingMetrics {
                close_records_stored: 0,
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

#[tokio::test]
async fn test_pay_for_quotes() {
    let (_anvil, network_token, mut payment_vault) = setup().await;

    let mut quote_payments = vec![];

    for _ in 0..MAX_TRANSFERS_PER_TRANSACTION {
        let quote_payment = random_quote_payment();
        quote_payments.push(quote_payment);
    }

    let _ = network_token
        .approve(*payment_vault.contract.address(), U256::MAX)
        .await
        .unwrap();

    // Contract provider has a different account coupled to it,
    // so we set it to the same as the network token contract
    payment_vault.set_provider(network_token.contract.provider().clone());

    let result = payment_vault.pay_for_quotes(quote_payments).await;

    assert!(result.is_ok(), "Failed with error: {:?}", result.err());
}
