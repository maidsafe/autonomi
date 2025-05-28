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
