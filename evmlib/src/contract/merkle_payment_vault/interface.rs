// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::contract::data_type_conversion;
use crate::merkle_batch_payment::CANDIDATES_PER_POOL;
use crate::quoting_metrics::QuotingMetrics;
use alloy::primitives::U256;
use alloy::sol;

// Generate bindings from ABI
sol!(
    #[allow(missing_docs)]
    #[derive(Debug)]
    #[sol(rpc)]
    IMerklePaymentVault,
    "abi/IMerklePaymentVault.json"
);

// Re-export contract instance type
pub use IMerklePaymentVault::IMerklePaymentVaultInstance;

// Re-export PoolHash (doesn't conflict with generated types)
pub use crate::merkle_batch_payment::PoolHash;

// Implement conversions from our API types to contract types
impl From<crate::merkle_batch_payment::PoolCommitment> for IMerklePaymentVault::PoolCommitment {
    fn from(pool: crate::merkle_batch_payment::PoolCommitment) -> Self {
        // Convert the exact-sized array directly
        let candidates_array: [IMerklePaymentVault::CandidateNode; CANDIDATES_PER_POOL] =
            pool.candidates.map(|c| c.into());

        Self {
            poolHash: pool.pool_hash.into(),
            candidates: candidates_array,
        }
    }
}

impl From<crate::merkle_batch_payment::CandidateNode> for IMerklePaymentVault::CandidateNode {
    fn from(node: crate::merkle_batch_payment::CandidateNode) -> Self {
        Self {
            rewardsAddress: node.rewards_address,
            metrics: node.metrics.into(),
        }
    }
}

impl From<QuotingMetrics> for IMerklePaymentVault::QuotingMetrics {
    fn from(metrics: QuotingMetrics) -> Self {
        Self {
            dataType: data_type_conversion(metrics.data_type),
            closeRecordsStored: U256::from(metrics.close_records_stored),
            recordsPerType: metrics
                .records_per_type
                .into_iter()
                .map(|(data_type, records)| IMerklePaymentVault::Record {
                    dataType: data_type_conversion(data_type),
                    records: U256::from(records),
                })
                .collect(),
        }
    }
}
