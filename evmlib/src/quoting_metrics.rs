// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::common::U256;
use serde::{Deserialize, Serialize};
use std::fmt::{Debug, Formatter, Result as FmtResult};

/// Quoting metrics used to generate a quote, or to track peer's status.
#[derive(Clone, Eq, PartialEq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct QuotingMetrics {
    /// DataTypes presented as its `index`
    pub data_type: u32,
    /// data size of the record
    pub data_size: usize,
    /// the records stored
    pub close_records_stored: usize,
    /// each entry to be `(data_type_index, num_of_records_of_that_type)`
    pub records_per_type: Vec<(u32, u32)>,
    /// the max_records configured
    pub max_records: usize,
    /// number of times that got paid
    pub received_payment_count: usize,
    /// the duration that node keeps connected to the network, measured in hours
    pub live_time: u64,
    /// network density from this node's perspective, which is the responsible_range as well
    /// This could be calculated via sampling, or equation calculation.
    pub network_density: Option<[u8; 32]>,
    /// estimated network size
    pub network_size: Option<u64>,
}

impl Debug for QuotingMetrics {
    fn fmt(&self, formatter: &mut Formatter) -> FmtResult {
        let density_u256 = self.network_density.map(U256::from_be_bytes);

        write!(
            formatter,
            "QuotingMetrics {{ data_type: {}, data_size: {}, close_records_stored: {}, records_per_type {:?}, max_records: {}, received_payment_count: {}, live_time: {}, network_density: {density_u256:?}, network_size: {:?} }}",
            self.data_type,
            self.data_size,
            self.close_records_stored,
            self.records_per_type,
            self.max_records,
            self.received_payment_count,
            self.live_time,
            self.network_size
        )
    }
}

impl QuotingMetrics {
    /// Convert to deterministic byte representation for hashing
    ///
    /// Uses fixed-width encoding (u64) for all numeric fields to ensure
    /// architecture-independent serialization across 32-bit and 64-bit platforms.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();

        bytes.extend_from_slice(&self.data_type.to_le_bytes());
        bytes.extend_from_slice(&(self.data_size as u64).to_le_bytes());
        bytes.extend_from_slice(&(self.close_records_stored as u64).to_le_bytes());
        bytes.extend_from_slice(&(self.records_per_type.len() as u32).to_le_bytes());
        for (dtype, count) in &self.records_per_type {
            bytes.extend_from_slice(&dtype.to_le_bytes());
            bytes.extend_from_slice(&count.to_le_bytes());
        }
        bytes.extend_from_slice(&(self.max_records as u64).to_le_bytes());
        bytes.extend_from_slice(&(self.received_payment_count as u64).to_le_bytes());
        bytes.extend_from_slice(&self.live_time.to_le_bytes());
        if let Some(density) = &self.network_density {
            bytes.push(1); // Option::Some marker
            bytes.extend_from_slice(density);
        } else {
            bytes.push(0); // Option::None marker
        }
        if let Some(size) = self.network_size {
            bytes.push(1); // Option::Some marker
            bytes.extend_from_slice(&size.to_le_bytes());
        } else {
            bytes.push(0); // Option::None marker
        }

        bytes
    }
}
