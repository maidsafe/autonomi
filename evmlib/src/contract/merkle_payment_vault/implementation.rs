// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::common::Address;
use alloy::network::Network;
use alloy::providers::Provider;
use alloy::sol;

sol!(
    #[allow(missing_docs)]
    #[sol(rpc)]
    MerklePaymentVaultImplementation,
    "artifacts/MerklePaymentVault.json"
);

/// Deploys the Merkle payment vault contract and returns the contract address
pub async fn deploy<P, N>(provider: &P, network_token_address: Address) -> Address
where
    P: Provider<N>,
    N: Network,
{
    let contract = MerklePaymentVaultImplementation::deploy(provider, network_token_address)
        .await
        .expect("Could not deploy Merkle payment vault implementation contract");

    *contract.address()
}
