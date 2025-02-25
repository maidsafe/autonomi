// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::{node::Node, Error, Result};
use ant_evm::{PaymentQuote, QuotingMetrics, RewardsAddress};
use ant_networking::Network;
use ant_protocol::{error::Error as ProtocolError, storage::ChunkAddress, NetworkAddress};
use libp2p::PeerId;
use std::time::Duration;
use xor_name::XorName;

impl Node {
    pub(crate) fn create_quote_for_storecost(
        network: &Network,
        address: &NetworkAddress,
        quoting_metrics: &QuotingMetrics,
        payment_address: &RewardsAddress,
    ) -> Result<PaymentQuote, ProtocolError> {
        let content = match address {
            NetworkAddress::ChunkAddress(addr) => *addr.xorname(),
            NetworkAddress::GraphEntryAddress(addr) => addr.xorname(),
            NetworkAddress::ScratchpadAddress(addr) => addr.xorname(),
            NetworkAddress::PointerAddress(addr) => addr.xorname(),
            NetworkAddress::PeerId(_) | NetworkAddress::RecordKey(_) => XorName::default(),
        };
        let timestamp = std::time::SystemTime::now();
        let bytes =
            PaymentQuote::bytes_for_signing(content, timestamp, quoting_metrics, payment_address);

        let Ok(signature) = network.sign(&bytes) else {
            return Err(ProtocolError::QuoteGenerationFailed);
        };

        let quote = PaymentQuote {
            content,
            timestamp,
            quoting_metrics: quoting_metrics.clone(),
            pub_key: network.get_pub_key(),
            rewards_address: *payment_address,
            signature,
        };

        Ok(quote)
    }
}

pub(crate) fn verify_quote_for_storecost(
    network: &Network,
    quote: PaymentQuote,
    address: &NetworkAddress,
) -> Result<()> {
    debug!("Verifying payment quote for {address:?}: {quote:?}");

    // check address
    let content = match address {
        NetworkAddress::ChunkAddress(addr) => *addr.xorname(),
        NetworkAddress::GraphEntryAddress(addr) => addr.xorname(),
        NetworkAddress::ScratchpadAddress(addr) => addr.xorname(),
        NetworkAddress::PointerAddress(addr) => addr.xorname(),
        NetworkAddress::PeerId(_) | NetworkAddress::RecordKey(_) => XorName::default(),
    };
    if content != quote.content {
        return Err(Error::InvalidQuoteContent);
    }

    // check if the quote has expired
    if quote.has_expired() {
        return Err(Error::QuoteExpired(address.clone()));
    }

    // check sig
    let bytes = quote.bytes_for_sig();
    let signature = quote.signature;
    if !network.verify(&bytes, &signature) {
        return Err(Error::InvalidQuoteSignature);
    }

    Ok(())
}

// Following metrics will be considered as client issue instead of node's bad quote.
//   1, quote is not regarding the same chunk as ours
//   2, quote is not around the same time as ours
//   3, quote is no longer valid
//
// Following metrics will be considered as node's bad quote.
//   1, QuoteMetrics doesn't match the historical quotes collected by self
pub(crate) async fn quotes_verification(network: &Network, quotes: Vec<(PeerId, PaymentQuote)>) {
    // Do nothing if self is not one of the quoters.
    if let Some((_, self_quote)) = quotes
        .iter()
        .find(|(peer_id, _quote)| *peer_id == network.peer_id())
    {
        let target_address =
            NetworkAddress::from_chunk_address(ChunkAddress::new(self_quote.content));
        if verify_quote_for_storecost(network, self_quote.clone(), &target_address).is_ok() {
            let quotes_for_nodes_duty: Vec<_> = quotes
                .iter()
                .filter(|(peer_id, quote)| {
                    let is_same_target = quote.content == self_quote.content;
                    let is_not_self = *peer_id != network.peer_id();

                    let time_gap = Duration::from_secs(10);
                    let is_around_same_time = if quote.timestamp > self_quote.timestamp {
                        self_quote.timestamp + time_gap > quote.timestamp
                    } else {
                        quote.timestamp + time_gap > self_quote.timestamp
                    };

                    let is_signed_by_the_claimed_peer =
                        quote.check_is_signed_by_claimed_peer(*peer_id);

                    is_same_target
                        && is_not_self
                        && is_around_same_time
                        && is_signed_by_the_claimed_peer
                })
                .cloned()
                .collect();

            // Pass down to swarm_driver level for further bad quote detection
            // against historical collected quotes.
            network.historical_verify_quotes(quotes_for_nodes_duty);
        }
    }
}
