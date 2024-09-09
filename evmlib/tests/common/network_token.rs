use alloy::network::{Ethereum, EthereumWallet};
use alloy::node_bindings::AnvilInstance;
use alloy::providers::fillers::{FillProvider, JoinFill, RecommendedFiller, WalletFiller};
use alloy::providers::{ProviderBuilder, ReqwestProvider};
use alloy::signers::local::PrivateKeySigner;
use alloy::transports::http::{Client, Http};
use evmlib::contract::network_token::NetworkToken;

#[allow(clippy::unwrap_used)]
pub async fn deploy_network_token_contract(
    anvil: &AnvilInstance,
) -> NetworkToken<
    Http<Client>,
    FillProvider<
        JoinFill<RecommendedFiller, WalletFiller<EthereumWallet>>,
        ReqwestProvider,
        Http<Client>,
        Ethereum,
    >,
    Ethereum,
> {
    // Set up signer from the first default Anvil account (Alice).
    let signer: PrivateKeySigner = anvil.keys()[0].clone().into();
    let wallet = EthereumWallet::from(signer);

    let rpc_url = anvil.endpoint().parse().unwrap();

    let provider = ProviderBuilder::new()
        .with_recommended_fillers()
        .wallet(wallet)
        .on_http(rpc_url);

    // Deploy the contract.
    NetworkToken::deploy(provider).await
}
