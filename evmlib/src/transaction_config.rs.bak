// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

#[derive(Clone, Debug, Default)]
pub struct TransactionConfig {
    pub max_fee_per_gas: MaxFeePerGas,
}

#[derive(Clone, Debug, Default)]
pub enum MaxFeePerGas {
    /// Use the current market price for fee per gas. WARNING: This can result in unexpected high gas fees!
    #[default]
    Auto,
    /// Use the current market price for fee per gas, but with an upper limit.
    LimitedAuto(u128),
    /// Use no max fee per gas. WARNING: This can result in unexpected high gas fees!
    Unlimited,
    /// Use a custom max fee per gas in WEI.
    Custom(u128),
}
