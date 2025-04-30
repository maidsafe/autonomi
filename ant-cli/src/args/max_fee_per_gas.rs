use autonomi::{MaxFeePerGas, Network};
use color_eyre::Help;

const AVERAGE_GAS_FEE_ARBITRUM_ONE: u128 = 15_000_000;
const AVERAGE_GAS_FEE_ARBITRUM_SEPOLIA: u128 = 100_000_000;

#[derive(Debug, Copy, Clone)]
pub enum MaxFeePerGasParam {
    Low,
    Market,
    Auto,
    LimitedAuto(u128),
    Unlimited,
    Custom(u128),
}

impl MaxFeePerGasParam {
    pub fn into_max_fee_per_gas(self, network: &Network) -> color_eyre::Result<MaxFeePerGas> {
        match self {
            Self::Low => Ok(MaxFeePerGas::LimitedAuto(
                Self::get_network_average_gas_fee(network)?,
            )),
            Self::Market => Ok(MaxFeePerGas::LimitedAuto(
                Self::get_network_average_gas_fee(network)? * 4,
            )),
            Self::Auto => Ok(MaxFeePerGas::Auto),
            Self::LimitedAuto(value) => Ok(MaxFeePerGas::LimitedAuto(value)),
            Self::Unlimited => Ok(MaxFeePerGas::Unlimited),
            Self::Custom(value) => Ok(MaxFeePerGas::Custom(value)),
        }
    }

    fn get_network_average_gas_fee(network: &Network) -> color_eyre::Result<u128> {
        match network {
            Network::ArbitrumOne => Ok(AVERAGE_GAS_FEE_ARBITRUM_ONE),
            Network::ArbitrumSepolia => Ok(AVERAGE_GAS_FEE_ARBITRUM_SEPOLIA),
            Network::ArbitrumSepoliaTest => Ok(AVERAGE_GAS_FEE_ARBITRUM_SEPOLIA),
            Network::Custom(_) => Err(
                color_eyre::eyre::eyre!("`--max-fee-per-gas` options `low` and `market` (default) are not supported when using a custom EVM network.")
                    .with_suggestion(|| "Try using a different `--max-fee-per-gas` option, such as `auto`, `limited-auto:<WEI AMOUNT>`, `unlimited`, or a custom value specified in WEI.")
            ),
        }
    }
}

impl std::str::FromStr for MaxFeePerGasParam {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.to_ascii_lowercase();

        match s.as_str() {
            "low" => Ok(MaxFeePerGasParam::Low),
            "market" => Ok(MaxFeePerGasParam::Market),
            "auto" => Ok(MaxFeePerGasParam::Auto),
            "unlimited" => Ok(MaxFeePerGasParam::Unlimited),
            _ => {
                if let Some(rest) = s.strip_prefix("limited-auto:") {
                    rest.parse::<u128>()
                        .map(MaxFeePerGasParam::LimitedAuto)
                        .map_err(|_| format!("Invalid limited-auto value: {rest}"))
                } else {
                    s.parse::<u128>()
                        .map(MaxFeePerGasParam::Custom)
                        .map_err(|_| format!("Invalid custom value: {s}"))
                }
            }
        }
    }
}

impl std::fmt::Display for MaxFeePerGasParam {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Low => write!(f, "MaxFeePerGas::Low"),
            Self::Market => write!(f, "MaxFeePerGas::Market"),
            Self::Auto => write!(f, "MaxFeePerGas::Market"),
            Self::LimitedAuto(value) => write!(f, "MaxFeePerGas::LimitedAuto({value})"),
            Self::Unlimited => write!(f, "MaxFeePerGas::Unlimited"),
            Self::Custom(value) => write!(f, "MaxFeePerGas::Custom({value})"),
        }
    }
}
