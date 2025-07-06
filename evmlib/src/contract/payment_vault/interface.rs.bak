use crate::common::{Address, Amount, QuoteHash, U256};
use crate::quoting_metrics::QuotingMetrics;
use alloy::primitives::FixedBytes;
use alloy::sol;

sol!(
    #[allow(missing_docs)]
    #[derive(Debug)]
    #[sol(rpc)]
    IPaymentVault,
    "abi/IPaymentVaultV2.json"
);

impl From<(QuoteHash, QuotingMetrics, Address)> for IPaymentVault::PaymentVerification {
    fn from(value: (QuoteHash, QuotingMetrics, Address)) -> Self {
        Self {
            metrics: value.1.into(),
            rewardsAddress: value.2,
            quoteHash: value.0,
        }
    }
}

impl From<(QuoteHash, Address, Amount)> for IPaymentVault::DataPayment {
    fn from(value: (QuoteHash, Address, Amount)) -> Self {
        Self {
            rewardsAddress: value.1,
            amount: value.2,
            quoteHash: value.0,
        }
    }
}

impl From<QuotingMetrics> for IPaymentVault::QuotingMetrics {
    fn from(value: QuotingMetrics) -> Self {
        Self {
            dataType: data_type_conversion(value.data_type),
            dataSize: U256::from(value.data_size),
            closeRecordsStored: U256::from(value.close_records_stored),
            recordsPerType: value
                .records_per_type
                .into_iter()
                .map(|(data_type, amount)| IPaymentVault::Record {
                    dataType: data_type_conversion(data_type),
                    records: U256::from(amount),
                })
                .collect(),
            maxRecords: U256::from(value.max_records),
            receivedPaymentCount: U256::from(value.received_payment_count),
            liveTime: U256::from(value.live_time),
            networkDensity: FixedBytes::<32>::from(value.network_density.unwrap_or_default())
                .into(),
            networkSize: value.network_size.map(U256::from).unwrap_or_default(),
        }
    }
}

fn data_type_conversion(data_type: u32) -> u8 {
    match data_type {
        0 => 2, // Chunk
        1 => 0, // GraphEntry
        2 => 3, // Pointer
        3 => 1, // Scratchpad
        _ => 4, // Does not exist
    }
}
