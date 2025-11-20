// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::common::Address;
use crate::contract::merkle_payment_vault::MerklePaymentVaultHandler;
use crate::contract::payment_vault::handler::PaymentVaultHandler;
use crate::contract::{merkle_payment_vault, network_token::NetworkToken, payment_vault};
use crate::reqwest::Url;
use crate::{CustomNetwork, Network};
use alloy::hex::ToHexExt;
use alloy::network::{Ethereum, EthereumWallet};
use alloy::node_bindings::{Anvil, AnvilInstance};
use alloy::providers::fillers::{
    BlobGasFiller, ChainIdFiller, FillProvider, GasFiller, JoinFill, NonceFiller,
    SimpleNonceManager, WalletFiller,
};
use alloy::providers::{Identity, ProviderBuilder, RootProvider};
use alloy::signers::local::PrivateKeySigner;

const ANVIL_DEFAULT_PORT: u16 = 61611;

pub struct Testnet {
    anvil: AnvilInstance,
    rpc_url: Url,
    network_token_address: Address,
    data_payments_address: Address,
    merkle_payments_address: Address,
}

impl Testnet {
    /// Starts an Anvil node and automatically deploys the network token and chunk payments smart contracts.
    pub async fn new() -> Self {
        let (node, rpc_url) = start_node();

        let network_token = deploy_network_token_contract(&rpc_url, &node).await;
        let data_payments =
            deploy_data_payments_contract(&rpc_url, &node, *network_token.contract.address()).await;
        let merkle_payments =
            deploy_merkle_payments_contract(&rpc_url, &node, *network_token.contract.address())
                .await;

        Testnet {
            anvil: node,
            rpc_url,
            network_token_address: *network_token.contract.address(),
            data_payments_address: *data_payments.contract.address(),
            merkle_payments_address: *merkle_payments.contract.address(),
        }
    }

    pub fn to_network(&self) -> Network {
        Network::Custom(CustomNetwork {
            rpc_url_http: self.rpc_url.clone(),
            payment_token_address: self.network_token_address,
            data_payments_address: self.data_payments_address,
            merkle_payments_address: Some(self.merkle_payments_address),
        })
    }

    pub fn default_wallet_private_key(&self) -> String {
        // Fetches private key from the first default Anvil account (Alice).
        let signer: PrivateKeySigner = self.anvil.keys()[0].clone().into();
        signer.to_bytes().encode_hex_with_prefix()
    }

    pub fn merkle_payments_address(&self) -> Address {
        self.merkle_payments_address
    }
}

/// Runs a local Anvil node bound to a specified IP address.
///
/// The `AnvilInstance` `endpoint` function is hardcoded to return "localhost", so we must also
/// return the RPC URL if we want to listen on a different address.
///
/// The `anvil` binary respects the `ANVIL_IP_ADDR` environment variable, but defaults to "localhost".
pub fn start_node() -> (AnvilInstance, Url) {
    let host = std::env::var("ANVIL_IP_ADDR").unwrap_or_else(|_| "localhost".to_string());
    let port = std::env::var("ANVIL_PORT")
        .unwrap_or(ANVIL_DEFAULT_PORT.to_string())
        .parse::<u16>()
        .expect("Invalid port number");

    let anvil = Anvil::new()
        .port(port)
        .try_spawn()
        .expect("Could not spawn Anvil node");

    // We have to manually return the RPC URL because the `anvil::endpoint_url()` always returns `localhost`
    let url = Url::parse(&format!("http://{host}:{}", anvil.port())).expect("Failed to parse URL");

    (anvil, url)
}

pub async fn deploy_network_token_contract(
    rpc_url: &Url,
    anvil: &AnvilInstance,
) -> NetworkToken<
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
> {
    // Set up signer from the first default Anvil account (Alice).
    let signer: PrivateKeySigner = anvil.keys()[0].clone().into();
    let wallet = EthereumWallet::from(signer);

    let provider = ProviderBuilder::new()
        .with_simple_nonce_management()
        .wallet(wallet)
        .connect_http(rpc_url.clone());

    // Deploy the contract.
    NetworkToken::deploy(provider).await
}

pub async fn deploy_data_payments_contract(
    rpc_url: &Url,
    anvil: &AnvilInstance,
    token_address: Address,
) -> PaymentVaultHandler<
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
> {
    // Set up signer from the second default Anvil account (Bob).
    let signer: PrivateKeySigner = anvil.keys()[1].clone().into();
    let wallet = EthereumWallet::from(signer);

    let provider = ProviderBuilder::new()
        .with_simple_nonce_management()
        .wallet(wallet)
        .connect_http(rpc_url.clone());

    // Deploy the contract.
    let payment_vault_contract_address =
        payment_vault::implementation::deploy(&provider, token_address).await;

    // Create a handler for the deployed contract
    PaymentVaultHandler::new(payment_vault_contract_address, provider)
}

pub async fn deploy_merkle_payments_contract(
    rpc_url: &Url,
    anvil: &AnvilInstance,
    token_address: Address,
) -> MerklePaymentVaultHandler<
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
> {
    // Set up signer from the third default Anvil account (Charlie).
    let signer: PrivateKeySigner = anvil.keys()[2].clone().into();
    let wallet = EthereumWallet::from(signer);

    let provider = ProviderBuilder::new()
        .with_simple_nonce_management()
        .wallet(wallet)
        .connect_http(rpc_url.clone());

    // Deploy the contract.
    let merkle_payment_vault_contract_address =
        merkle_payment_vault::implementation::deploy(&provider, token_address).await;

    // Create a handler for the deployed contract
    MerklePaymentVaultHandler::new(merkle_payment_vault_contract_address, provider)
}

#[cfg(test)]
mod tests {
    use crate::testnet::Testnet;

    #[tokio::test]
    async fn test_run_multiple_testnets_in_parallel() {
        let _testnet_1 = Testnet::new().await;
        let _testnet_2 = Testnet::new().await;
        let _testnet_3 = Testnet::new().await;
        let _testnet_4 = Testnet::new().await;
    }
}
