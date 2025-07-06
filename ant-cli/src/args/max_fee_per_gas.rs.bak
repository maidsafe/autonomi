// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

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
    fn get_network_average_gas_fee(network: &Network) -> color_eyre::Result<u128> {
        match network {
            Network::ArbitrumOne => Ok(AVERAGE_GAS_FEE_ARBITRUM_ONE),
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

pub fn get_max_fee_per_gas_from_opt_param(
    param: Option<MaxFeePerGasParam>,
    network: &Network,
) -> color_eyre::Result<MaxFeePerGas> {
    let param = match (param, network) {
        (None, Network::Custom(_)) => MaxFeePerGasParam::Auto,
        (None, _) => MaxFeePerGasParam::Market,
        (Some(p), _) => p,
    };

    match param {
        MaxFeePerGasParam::Low => Ok(MaxFeePerGas::LimitedAuto(
            MaxFeePerGasParam::get_network_average_gas_fee(network)?,
        )),
        MaxFeePerGasParam::Market => Ok(MaxFeePerGas::LimitedAuto(
            MaxFeePerGasParam::get_network_average_gas_fee(network)? * 4,
        )),
        MaxFeePerGasParam::Auto => Ok(MaxFeePerGas::Auto),
        MaxFeePerGasParam::LimitedAuto(value) => Ok(MaxFeePerGas::LimitedAuto(value)),
        MaxFeePerGasParam::Unlimited => Ok(MaxFeePerGas::Unlimited),
        MaxFeePerGasParam::Custom(value) => Ok(MaxFeePerGas::Custom(value)),
    }
}
