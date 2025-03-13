// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::EvmError;
use evmlib::{
    common::{Address as RewardsAddress, QuoteHash},
    quoting_metrics::QuotingMetrics,
};
use libp2p::{identity::PublicKey, Multiaddr, PeerId};
use serde::{Deserialize, Serialize};
pub use std::time::SystemTime;
use xor_name::XorName;

/// The time in seconds that a quote is valid for
pub const QUOTE_EXPIRATION_SECS: u64 = 3600;

/// The margin allowed for live_time
const LIVE_TIME_MARGIN: u64 = 10;

#[derive(Debug, Clone, PartialEq, Eq, Ord, PartialOrd, Serialize, Deserialize)]
pub struct EncodedPeerId(Vec<u8>);

impl EncodedPeerId {
    pub fn to_peer_id(&self) -> Result<PeerId, libp2p::identity::ParseError> {
        PeerId::from_bytes(&self.0)
    }
}

impl From<PeerId> for EncodedPeerId {
    fn from(peer_id: PeerId) -> Self {
        let bytes = peer_id.to_bytes();
        EncodedPeerId(bytes)
    }
}

/// The proof of payment for a data payment
#[derive(Debug, Clone, PartialEq, Eq, Ord, PartialOrd, Serialize, Deserialize)]
pub struct ProofOfPayment {
    pub peer_quotes: Vec<(EncodedPeerId, Vec<Multiaddr>, PaymentQuote)>,
}

impl ProofOfPayment {
    /// returns a short digest of the proof of payment to use for verification
    pub fn digest(&self) -> Vec<(QuoteHash, QuotingMetrics, RewardsAddress)> {
        self.peer_quotes
            .clone()
            .into_iter()
            .map(|(_, _, quote)| (quote.hash(), quote.quoting_metrics, quote.rewards_address))
            .collect()
    }

    /// returns the list of payees
    pub fn payees(&self) -> Vec<(PeerId, Vec<Multiaddr>)> {
        self.peer_quotes
            .iter()
            .filter_map(|(peer_id, addrs, _)| {
                if let Ok(peer_id) = peer_id.to_peer_id() {
                    Some((peer_id, addrs.clone()))
                } else {
                    None
                }
            })
            .collect()
    }

    /// has the quote expired
    pub fn has_expired(&self) -> bool {
        self.peer_quotes
            .iter()
            .any(|(_, _, quote)| quote.has_expired())
    }

    /// Returns all quotes by given peer id
    pub fn quotes_by_peer(&self, peer_id: &PeerId) -> Vec<&PaymentQuote> {
        self.peer_quotes
            .iter()
            .filter_map(|(_id, _addr, quote)| {
                if let Ok(quote_peer_id) = quote.peer_id() {
                    if *peer_id == quote_peer_id {
                        return Some(quote);
                    }
                }
                None
            })
            .collect()
    }

    /// verifies the proof of payment is valid for the given peer id
    pub fn verify_for(&self, peer_id: PeerId) -> bool {
        // make sure I am in the list of payees
        if !self.payees().iter().any(|(id, _addrs)| *id == peer_id) {
            warn!("Payment does not contain node peer id");
            debug!("Payment contains peer ids: {:?}", self.payees());
            debug!("Node peer id: {:?}", peer_id);
            return false;
        }

        // verify all signatures
        for (encoded_peer_id, _addr, quote) in self.peer_quotes.iter() {
            let peer_id = match encoded_peer_id.to_peer_id() {
                Ok(peer_id) => peer_id,
                Err(e) => {
                    warn!("Invalid encoded peer id: {e}");
                    return false;
                }
            };
            if !quote.check_is_signed_by_claimed_peer(peer_id) {
                warn!("Payment is not signed by claimed peer");
                return false;
            }
        }
        true
    }

    /// Verifies whether all quotes were made for the expected data type.
    pub fn verify_data_type(&self, data_type: u32) -> bool {
        for (_, _, quote) in self.peer_quotes.iter() {
            if quote.quoting_metrics.data_type != data_type {
                return false;
            }
        }

        true
    }
}

/// A payment quote to store data given by a node to a client
/// Note that the PaymentQuote is a contract between the node and itself to make sure the clients aren’t mispaying.
/// It is NOT a contract between the client and the node.
#[derive(Clone, Eq, PartialEq, PartialOrd, Ord, Serialize, Deserialize, custom_debug::Debug)]
pub struct PaymentQuote {
    /// the content paid for
    pub content: XorName,
    /// the local node time when the quote was created
    pub timestamp: SystemTime,
    /// quoting metrics being used to generate this quote
    pub quoting_metrics: QuotingMetrics,
    /// the node's wallet address
    pub rewards_address: RewardsAddress,
    /// the node's libp2p identity public key in bytes (PeerId)
    #[debug(skip)]
    pub pub_key: Vec<u8>,
    /// the node's signature for the quote
    #[debug(skip)]
    pub signature: Vec<u8>,
}

impl PaymentQuote {
    pub fn hash(&self) -> QuoteHash {
        let mut bytes = self.bytes_for_sig();
        bytes.extend_from_slice(self.pub_key.as_slice());
        bytes.extend_from_slice(self.signature.as_slice());
        evmlib::cryptography::hash(bytes)
    }

    /// returns the bytes to be signed from the given parameters
    pub fn bytes_for_signing(
        xorname: XorName,
        timestamp: SystemTime,
        quoting_metrics: &QuotingMetrics,
        rewards_address: &RewardsAddress,
    ) -> Vec<u8> {
        let mut bytes = xorname.to_vec();
        bytes.extend_from_slice(
            &timestamp
                .duration_since(SystemTime::UNIX_EPOCH)
                .expect("Unix epoch to be in the past")
                .as_secs()
                .to_le_bytes(),
        );
        let serialised_quoting_metrics = rmp_serde::to_vec(quoting_metrics).unwrap_or_default();
        bytes.extend_from_slice(&serialised_quoting_metrics);
        bytes.extend_from_slice(rewards_address.as_slice());
        bytes
    }

    /// Returns the bytes to be signed from self
    pub fn bytes_for_sig(&self) -> Vec<u8> {
        Self::bytes_for_signing(
            self.content,
            self.timestamp,
            &self.quoting_metrics,
            &self.rewards_address,
        )
    }

    /// Returns the peer id of the node that created the quote
    pub fn peer_id(&self) -> Result<PeerId, EvmError> {
        if let Ok(pub_key) = libp2p::identity::PublicKey::try_decode_protobuf(&self.pub_key) {
            Ok(PeerId::from(pub_key.clone()))
        } else {
            error!("Can't parse PublicKey from protobuf");
            Err(EvmError::InvalidQuotePublicKey)
        }
    }

    /// Check self is signed by the claimed peer
    pub fn check_is_signed_by_claimed_peer(&self, claimed_peer: PeerId) -> bool {
        let pub_key = if let Ok(pub_key) = PublicKey::try_decode_protobuf(&self.pub_key) {
            pub_key
        } else {
            error!("Can't parse PublicKey from protobuf");
            return false;
        };

        let self_peer_id = PeerId::from(pub_key.clone());

        if self_peer_id != claimed_peer {
            error!("This quote {self:?} of {self_peer_id:?} is not signed by {claimed_peer:?}");
            return false;
        }

        let bytes = self.bytes_for_sig();

        if !pub_key.verify(&bytes, &self.signature) {
            error!("Signature is not signed by claimed pub_key");
            return false;
        }

        true
    }

    /// Returns true if the quote has expired
    pub fn has_expired(&self) -> bool {
        let now = SystemTime::now();

        let dur_s = match now.duration_since(self.timestamp) {
            Ok(dur) => dur.as_secs(),
            Err(_) => return true,
        };
        dur_s > QUOTE_EXPIRATION_SECS
    }

    /// test utility to create a dummy quote
    #[cfg(test)]
    pub fn test_dummy(xorname: XorName) -> Self {
        use evmlib::utils::dummy_address;

        Self {
            content: xorname,
            timestamp: SystemTime::now(),
            quoting_metrics: QuotingMetrics {
                data_size: 0,
                data_type: 0,
                close_records_stored: 0,
                records_per_type: vec![],
                max_records: 0,
                received_payment_count: 0,
                live_time: 0,
                network_density: None,
                network_size: None,
            },
            pub_key: vec![],
            signature: vec![],
            rewards_address: dummy_address(),
        }
    }

    /// Check whether self is newer than the target quote.
    pub fn is_newer_than(&self, other: &Self) -> bool {
        self.timestamp > other.timestamp
    }

    /// Check against a new quote, verify whether it is a valid one from self perspective.
    /// Returns `true` to flag the `other` quote is valid, from self perspective.
    pub fn historical_verify(&self, other: &Self) -> bool {
        // There is a chance that an old quote got used later than a new quote
        let self_is_newer = self.is_newer_than(other);
        let (old_quote, new_quote) = if self_is_newer {
            (other, self)
        } else {
            (self, other)
        };

        if new_quote.quoting_metrics.live_time < old_quote.quoting_metrics.live_time {
            info!("Claimed live_time out of sequence");
            return false;
        }

        // TODO: Double check if this applies, as this will prevent a node restart with same ID
        if new_quote.quoting_metrics.received_payment_count
            < old_quote.quoting_metrics.received_payment_count
        {
            info!("claimed received_payment_count out of sequence");
            return false;
        }

        let old_elapsed = if let Ok(elapsed) = old_quote.timestamp.elapsed() {
            elapsed
        } else {
            // The elapsed call could fail due to system clock change
            // hence consider the verification succeeded.
            info!("old_quote timestamp elapsed call failure");
            return true;
        };
        let new_elapsed = if let Ok(elapsed) = new_quote.timestamp.elapsed() {
            elapsed
        } else {
            // The elapsed call could fail due to system clock change
            // hence consider the verification succeeded.
            info!("new_quote timestamp elapsed call failure");
            return true;
        };

        let time_diff = old_elapsed.as_secs().saturating_sub(new_elapsed.as_secs());
        let live_time_diff =
            new_quote.quoting_metrics.live_time - old_quote.quoting_metrics.live_time;
        // In theory, these two shall match, give it a LIVE_TIME_MARGIN to avoid system glitch
        if live_time_diff > time_diff + LIVE_TIME_MARGIN {
            info!("claimed live_time out of sync with the timestamp");
            return false;
        }

        // There could be pruning to be undertaken, also the close range keeps changing as well.
        // Hence `close_records_stored` could be growing or shrinking.
        // Currently not to carry out check on it, just logging to observe the trend.
        debug!(
            "The new quote has {} close records stored, meanwhile old one has {}.",
            new_quote.quoting_metrics.close_records_stored,
            old_quote.quoting_metrics.close_records_stored
        );

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use libp2p::identity::Keypair;
    use std::{thread::sleep, time::Duration};

    #[test]
    fn test_encode_decode_peer_id() {
        let id = PeerId::random();
        let encoded = EncodedPeerId::from(id);
        let decoded = encoded.to_peer_id().expect("decode to work");
        assert_eq!(id, decoded);
    }

    #[test]
    fn test_is_newer_than() {
        let old_quote = PaymentQuote::test_dummy(Default::default());
        sleep(Duration::from_millis(100));
        let new_quote = PaymentQuote::test_dummy(Default::default());
        assert!(new_quote.is_newer_than(&old_quote));
        assert!(!old_quote.is_newer_than(&new_quote));
    }

    #[test]
    fn test_is_signed_by_claimed_peer() {
        let keypair = Keypair::generate_ed25519();
        let peer_id = keypair.public().to_peer_id();

        let false_peer = PeerId::random();

        let mut quote = PaymentQuote::test_dummy(Default::default());
        let bytes = quote.bytes_for_sig();
        let signature = if let Ok(sig) = keypair.sign(&bytes) {
            sig
        } else {
            panic!("Cannot sign the quote!");
        };

        // Check failed with both incorrect pub_key and signature
        assert!(!quote.check_is_signed_by_claimed_peer(peer_id));
        assert!(!quote.check_is_signed_by_claimed_peer(false_peer));

        // Check failed with correct pub_key but incorrect signature
        quote.pub_key = keypair.public().encode_protobuf();
        assert!(!quote.check_is_signed_by_claimed_peer(peer_id));
        assert!(!quote.check_is_signed_by_claimed_peer(false_peer));

        // Check succeed with correct pub_key and signature,
        // and failed with incorrect claimed signer (peer)
        quote.signature = signature;
        assert!(quote.check_is_signed_by_claimed_peer(peer_id));
        assert!(!quote.check_is_signed_by_claimed_peer(false_peer));

        // Check failed with incorrect pub_key but correct signature
        quote.pub_key = Keypair::generate_ed25519().public().encode_protobuf();
        assert!(!quote.check_is_signed_by_claimed_peer(peer_id));
        assert!(!quote.check_is_signed_by_claimed_peer(false_peer));
    }

    #[test]
    fn test_historical_verify() {
        let mut old_quote = PaymentQuote::test_dummy(Default::default());
        sleep(Duration::from_millis(100));
        let mut new_quote = PaymentQuote::test_dummy(Default::default());

        // historical_verify will swap quotes to compare based on timeline automatically
        assert!(new_quote.historical_verify(&old_quote));
        assert!(old_quote.historical_verify(&new_quote));

        // Out of sequence received_payment_count shall be detected
        old_quote.quoting_metrics.received_payment_count = 10;
        new_quote.quoting_metrics.received_payment_count = 9;
        assert!(!new_quote.historical_verify(&old_quote));
        assert!(!old_quote.historical_verify(&new_quote));
        // Reset to correct one
        new_quote.quoting_metrics.received_payment_count = 11;
        assert!(new_quote.historical_verify(&old_quote));
        assert!(old_quote.historical_verify(&new_quote));

        // Out of sequence live_time shall be detected
        new_quote.quoting_metrics.live_time = 10;
        old_quote.quoting_metrics.live_time = 11;
        assert!(!new_quote.historical_verify(&old_quote));
        assert!(!old_quote.historical_verify(&new_quote));
        // Out of margin live_time shall be detected
        new_quote.quoting_metrics.live_time = 11 + LIVE_TIME_MARGIN + 1;
        assert!(!new_quote.historical_verify(&old_quote));
        assert!(!old_quote.historical_verify(&new_quote));
        // Reset live_time to be within the margin
        new_quote.quoting_metrics.live_time = 11 + LIVE_TIME_MARGIN - 1;
        assert!(new_quote.historical_verify(&old_quote));
        assert!(old_quote.historical_verify(&new_quote));
    }
}
