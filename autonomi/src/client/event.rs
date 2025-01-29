// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use ant_evm::Amount;
use serde::Serialize;
use xor_name::XorName;

/// Events that can be broadcasted by the client.
#[derive(Debug, Clone)]
pub enum ClientEvent {
    /// Summary of a payment operation. This event is not emitted if the payment fails.
    PaymentSucceeded(PaymentSummary),
    /// The record has been uploaded to the network. This event is not emitted if the record was
    /// already present on the network. Refer to [`Self::DataAlreadyPresent`] for that case.
    UploadSucceeded(XorName),
    /// The record is already present on the network. No payment was made for this record.
    DataAlreadyPresent(XorName),
    /// Failed to upload the record to the network.
    UploadFailed(XorName),
    /// The record has been downloaded from the network.
    DownloadSucceeded(XorName),
    /// Failed to download the record from the network.
    DownloadFailed(XorName),
}

/// Summary of a payment operation.
/// This event is emitted even if the payment fails.
#[derive(Debug, Clone, Serialize)]
pub struct PaymentSummary {
    /// Amount of tokens that were spent.
    pub tokens_spent: Amount,
    /// Records that were paid for.
    pub records_paid: usize,
    /// Records that were already present on the network. No payment were made for these.
    pub records_already_paid: usize,
}
