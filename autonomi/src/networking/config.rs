use crate::networking::{PeerId, Quorum, Record};
use ant_protocol::messages::{ChunkProof, Nonce};
use ant_protocol::PrettyPrintRecordKey;
use core::fmt;
use exponential_backoff::Backoff;
use std::collections::HashSet;
use std::fmt::Debug;
use std::num::NonZeroUsize;
use std::time::Duration;

/// A strategy that translates into a configuration for exponential backoff.
/// The first retry is done after 2 seconds, after which the backoff is roughly doubled each time.
/// The interval does not go beyond 32 seconds. So the intervals increase from 2 to 4, to 8, to 16, to 32 seconds and
/// all attempts are made at most 32 seconds apart.
///
/// The exact timings depend on jitter, which is set to 0.2, meaning the intervals can deviate quite a bit
/// from the ones listed in the docs.
///
/// The default strategy is `Balanced`.
#[derive(Clone, Debug, Copy, Default)]
pub enum RetryStrategy {
    /// Attempt once (no retries)
    None,
    /// Retry 3 times (waits 2s, 4s and lastly 8s; max total time ~14s)
    Quick,
    /// Retry 5 times (waits 2s, 4s, 8s, 16s and lastly 32s; max total time ~62s)
    #[default]
    Balanced,
    /// Retry 9 times (waits 2s, 4s, 8s, 16s, 32s, 32s, 32s, 32s and lastly 32s; max total time ~190s)
    Persistent,
    /// Attempt a specific number of times
    N(NonZeroUsize),
}

impl RetryStrategy {
    pub fn attempts(&self) -> usize {
        match self {
            RetryStrategy::None => 1,
            RetryStrategy::Quick => 4,
            RetryStrategy::Balanced => 6,
            RetryStrategy::Persistent => 10,
            RetryStrategy::N(x) => x.get(),
        }
    }

    pub fn backoff(&self) -> Backoff {
        let mut backoff = Backoff::new(
            self.attempts() as u32,
            Duration::from_secs(1), // First interval is double of this (see https://github.com/yoshuawuyts/exponential-backoff/issues/23)
            Some(Duration::from_secs(8)),
        );
        backoff.set_factor(2); // Default.
        backoff.set_jitter(0.2); // Default is 0.3.
        backoff
    }
}

impl fmt::Display for RetryStrategy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

/// The various settings to apply to when fetching a record from network
#[derive(Clone)]
pub struct GetRecordCfg {
    /// The query will result in an error if we get records less than the provided Quorum
    pub get_quorum: Quorum,
    /// If enabled, the provided `RetryStrategy` is used to retry if a GET attempt fails.
    pub retry_strategy: RetryStrategy,
    /// Only return if we fetch the provided record.
    pub target_record: Option<Record>,
    /// Logs if the record was not fetched from the provided set of peers.
    pub expected_holders: HashSet<PeerId>,
}

impl GetRecordCfg {
    pub fn does_target_match(&self, record: &Record) -> bool {
        if let Some(ref target_record) = self.target_record {
            target_record == record
        } else {
            // Not have target_record to check with
            true
        }
    }
}

impl Debug for GetRecordCfg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut f = f.debug_struct("GetRecordCfg");
        f.field("get_quorum", &self.get_quorum)
            .field("retry_strategy", &self.retry_strategy);

        match &self.target_record {
            Some(record) => {
                let pretty_key = PrettyPrintRecordKey::from(&record.key);
                f.field("target_record", &pretty_key);
            }
            None => {
                f.field("target_record", &"None");
            }
        };

        f.field("expected_holders", &self.expected_holders).finish()
    }
}

/// The various settings related to writing a record to the network.
#[derive(Debug, Clone)]
pub struct PutRecordCfg {
    /// The quorum used by KAD PUT. KAD still sends out the request to all the peers set by the `replication_factor`, it
    /// just makes sure that we get atleast `n` successful responses defined by the Quorum.
    /// Our nodes currently send `Ok()` response for every KAD PUT. Thus this field does not do anything atm.
    pub put_quorum: Quorum,
    /// If enabled, the provided `RetryStrategy` is used to retry if a PUT attempt fails.
    pub retry_strategy: RetryStrategy,
    /// Use the `kad::put_record_to` to PUT the record only to the specified peers. If this option is set to None, we
    /// will be using `kad::put_record` which would PUT the record to all the closest members of the record.
    pub use_put_record_to: Option<Vec<PeerId>>,
    /// Enables verification after writing. The VerificationKind is used to determine the method to use.
    pub verification: Option<(VerificationKind, GetRecordCfg)>,
}

/// The methods in which verification on a PUT can be carried out.
#[derive(Debug, Clone)]
pub enum VerificationKind {
    /// Uses the default KAD GET to perform verification.
    Network,
    /// Uses the default KAD GET to perform verification, but don't error out on split records
    Crdt,
    /// Uses the hash based verification for chunks.
    ChunkProof {
        expected_proof: ChunkProof,
        nonce: Nonce,
    },
}
