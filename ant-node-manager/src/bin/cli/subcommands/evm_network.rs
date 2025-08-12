// Copyright (C) 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use ant_evm::{EvmNetwork, get_evm_network};
use clap::Subcommand;
use color_eyre::Result;

#[derive(Subcommand, Clone, Debug)]
#[allow(clippy::enum_variant_names)]
pub enum EvmNetworkCommand {
    /// Use the Arbitrum One network
    EvmArbitrumOne,

    /// Use the Arbitrum Sepolia network with test contracts
    EvmArbitrumSepoliaTest,

    /// Use a custom network
    EvmCustom {
        /// The RPC URL for the custom network
        #[arg(long)]
        rpc_url: String,

        /// The payment token contract address
        #[arg(long, short)]
        payment_token_address: String,

        /// The chunk payments contract address
        #[arg(long, short)]
        data_payments_address: String,
    },

    /// Use the local EVM testnet, loaded from a CSV file.
    EvmLocal,
}

impl TryInto<EvmNetwork> for EvmNetworkCommand {
    type Error = color_eyre::eyre::Error;

    fn try_into(self) -> Result<EvmNetwork> {
        match self {
            Self::EvmArbitrumOne => Ok(EvmNetwork::ArbitrumOne),
            Self::EvmArbitrumSepoliaTest => Ok(EvmNetwork::ArbitrumSepoliaTest),
            Self::EvmLocal => {
                let network = get_evm_network(true, None)?;
                Ok(network)
            }
            Self::EvmCustom {
                rpc_url,
                payment_token_address,
                data_payments_address,
            } => Ok(EvmNetwork::new_custom(
                &rpc_url,
                &payment_token_address,
                &data_payments_address,
            )),
        }
    }
}
