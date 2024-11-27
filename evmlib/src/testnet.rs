// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::common::Address;
use crate::contract::data_payments::DataPaymentsHandler;
use crate::contract::network_token::NetworkToken;
use crate::reqwest::Url;
use crate::{CustomNetwork, Network};
use alloy::hex::ToHexExt;
use alloy::network::{Ethereum, EthereumWallet};
use alloy::node_bindings::{Anvil, AnvilInstance};
use alloy::providers::fillers::{
    BlobGasFiller, ChainIdFiller, FillProvider, GasFiller, JoinFill, NonceFiller, WalletFiller,
};
use alloy::providers::{Identity, ProviderBuilder, ReqwestProvider};
use alloy::signers::local::PrivateKeySigner;
use alloy::transports::http::{Client, Http};

pub struct Testnet {
    anvil: AnvilInstance,
    rpc_url: Url,
    network_token_address: Address,
    data_payments_address: Address,
}

impl Testnet {
    /// Starts an Anvil node and automatically deploys the network token and chunk payments smart contracts.
    pub async fn new() -> Self {
        let (anvil, rpc_url) = start_node();

        let network_token = deploy_network_token_contract(&rpc_url, &anvil).await;
        let data_payments =
            deploy_data_payments_contract(&rpc_url, &anvil, *network_token.contract.address())
                .await;
        debug!("Instantiated a testnet");
        Testnet {
            anvil,
            rpc_url,
            network_token_address: *network_token.contract.address(),
            data_payments_address: *data_payments.contract.address(),
        }
    }

    pub fn to_network(&self) -> Network {
        Network::Custom(CustomNetwork {
            rpc_url_http: self.rpc_url.clone(),
            payment_token_address: self.network_token_address,
            data_payments_address: self.data_payments_address,
        })
    }

    pub fn default_wallet_private_key(&self) -> String {
        // Fetches private key from the first default Anvil account (Alice).
        let signer: PrivateKeySigner = self.anvil.keys()[0].clone().into();
        signer.to_bytes().encode_hex_with_prefix()
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
        .unwrap_or_else(|_| "4343".to_string())
        .parse::<u16>()
        .expect("Invalid port number");

    let anvil = Anvil::new()
        .port(port)
        .try_spawn()
        .expect("Could not spawn Anvil node");

    let url = Url::parse(&format!("http://{host}:{port}")).expect("Failed to parse URL");
    debug!("Started Anvil node at {url}");
    (anvil, url)
}

pub async fn deploy_network_token_contract(
    rpc_url: &Url,
    anvil: &AnvilInstance,
) -> NetworkToken<
    Http<Client>,
    FillProvider<
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
    >,
    Ethereum,
> {
    // Set up signer from the first default Anvil account (Alice).
    let signer: PrivateKeySigner = anvil.keys()[0].clone().into();
    let wallet = EthereumWallet::from(signer);

    let provider = ProviderBuilder::new()
        .with_recommended_fillers()
        .wallet(wallet)
        .on_http(rpc_url.clone());

    // Deploy the contract.
    let deployed = NetworkToken::deploy(provider).await;
    let address = *deployed.contract.address();
    debug!("Deployed network token contract at {address}");
    deployed
}

pub async fn deploy_data_payments_contract(
    rpc_url: &Url,
    anvil: &AnvilInstance,
    token_address: Address,
) -> DataPaymentsHandler<
    Http<Client>,
    FillProvider<
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
    >,
    Ethereum,
> {
    // Set up signer from the second default Anvil account (Bob).
    let signer: PrivateKeySigner = anvil.keys()[1].clone().into();
    let wallet = EthereumWallet::from(signer);

    let provider = ProviderBuilder::new()
        .with_recommended_fillers()
        .wallet(wallet)
        .on_http(rpc_url.clone());

    // Deploy the contract.
    let deployed = DataPaymentsHandler::deploy(provider, token_address).await;
    let address = *deployed.contract.address();
    debug!("Deployed data payments contract at {address}");
    deployed
}
