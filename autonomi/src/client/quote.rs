// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::Client;

// Re-export types from autonomi_core
pub use ant_protocol::storage::DataTypes;
pub use autonomi_core::{
    Addresses, CostError,
    client::quote::{QuoteForAddress, StoreQuote},
};

use ant_evm::PaymentQuote;
use libp2p::PeerId;
use xor_name::XorName;

impl Client {
    /// Get raw quotes from nodes.
    /// These quotes do not include actual record prices.
    /// You will likely want to use `get_store_quotes` instead.
    pub async fn get_raw_quotes(
        &self,
        data_type: DataTypes,
        content_addrs: impl Iterator<Item = (XorName, usize)>,
    ) -> Vec<Result<(XorName, Vec<(PeerId, Addresses, PaymentQuote)>), CostError>> {
        self.core_client
            .get_raw_quotes(data_type, content_addrs)
            .await
    }

    pub async fn get_store_quotes(
        &self,
        data_type: DataTypes,
        content_addrs: impl Iterator<Item = (XorName, usize)>,
    ) -> Result<StoreQuote, CostError> {
        self.core_client
            .get_store_quotes(data_type, content_addrs)
            .await
    }
}
