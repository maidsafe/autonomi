// Copyright 2023 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use std::{collections::BTreeMap, time::SystemTime};

use serde::{Deserialize, Serialize};
use xor_name::XorName;

use crate::{MainPubkey, NanoTokens, Transfer};

#[derive(Clone, Serialize, Deserialize, Eq, PartialEq, custom_debug::Debug)]
pub struct Payment {
    /// The transfers we make
    #[debug(skip)]
    pub transfers: Vec<Transfer>,
    /// The Quote we're paying for
    pub quote: PaymentQuote,
}

/// Information relating to a data payment for one address
#[derive(Clone, Serialize, Deserialize)]
pub struct PaymentDetails {
    /// The node we pay
    pub recipient: MainPubkey,
    /// The transfer we send to it and its amount as reference
    pub transfer: (Transfer, NanoTokens),
    /// The network Royalties
    pub royalties: (Transfer, NanoTokens),
    /// The original quote
    pub quote: PaymentQuote,
}

impl PaymentDetails {
    /// create a Payment for a PaymentDetails
    pub fn to_payment(&self) -> Payment {
        Payment {
            transfers: vec![self.transfer.0.clone(), self.royalties.0.clone()],
            quote: self.quote.clone(),
        }
    }
}

/// A map of content to their payments
pub type ContentPaymentsMap = BTreeMap<XorName, PaymentDetails>;

/// A generic type for signatures
pub type QuoteSignature = Vec<u8>;

/// A payment quote to store data given by a node to a client
/// Note that the PaymentQuote is a contract between the node and itself to make sure the clients aren’t mispaying.
/// It is NOT a contract between the client and the node.
#[derive(
    Clone, Eq, PartialEq, PartialOrd, Ord, Hash, Serialize, Deserialize, custom_debug::Debug,
)]
pub struct PaymentQuote {
    /// the content paid for
    pub content: XorName,
    /// how much the node demands for storing the content
    pub cost: NanoTokens,
    /// the local node time when the quote was created
    pub timestamp: SystemTime,
    /// the node's signature of the 3 fields above
    #[debug(skip)]
    pub signature: QuoteSignature,
}

impl PaymentQuote {
    /// create an empty PaymentQuote
    pub fn zero() -> Self {
        Self {
            content: Default::default(),
            cost: NanoTokens::zero(),
            timestamp: SystemTime::now(),
            signature: vec![],
        }
    }

    /// returns the bytes to be signed
    pub fn bytes_for_signing(xorname: XorName, cost: NanoTokens, timestamp: SystemTime) -> Vec<u8> {
        let mut bytes = xorname.to_vec();
        bytes.extend_from_slice(&cost.to_bytes());
        bytes.extend_from_slice(
            &timestamp
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_secs()
                .to_le_bytes(),
        );
        bytes
    }

    /// test utility to create a dummy quote
    pub fn test_dummy(xorname: XorName, cost: NanoTokens) -> Self {
        Self {
            content: xorname,
            cost,
            timestamp: SystemTime::now(),
            signature: vec![],
        }
    }
}
