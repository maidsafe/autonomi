use ant_evm::EvmNetwork;
use clap::Subcommand;

#[derive(Subcommand, Clone, Debug)]
#[allow(clippy::enum_variant_names)]
pub(crate) enum EvmNetworkCommand {
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
}

#[allow(clippy::from_over_into)]
impl Into<EvmNetwork> for EvmNetworkCommand {
    fn into(self) -> EvmNetwork {
        match self {
            Self::EvmArbitrumOne => EvmNetwork::ArbitrumOne,
            Self::EvmArbitrumSepoliaTest => EvmNetwork::ArbitrumSepoliaTest,
            Self::EvmCustom {
                rpc_url,
                payment_token_address,
                data_payments_address,
            } => EvmNetwork::new_custom(&rpc_url, &payment_token_address, &data_payments_address),
        }
    }
}
