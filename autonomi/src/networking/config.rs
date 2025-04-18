// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use exponential_backoff::Backoff;
use std::fmt::Debug;
use std::time::Duration;

use super::Quorum;

/// A strategy that translates into a configuration for exponential backoff.
/// The first retry is done after 2 seconds, after which the backoff is roughly doubled each time.
/// The interval does not go beyond 8 seconds. So the intervals increase from 2 to 4, to 8 seconds and
/// all attempts are made at most 8 seconds apart.
///
/// The default strategy is `Balanced`.
#[derive(Clone, Debug, Copy, Default)]
#[repr(usize)]
pub enum RetryStrategy {
    /// Attempt once (no retries)
    None = 1,
    /// Try 4 times (waits 2s, 4s, 8s; max total sleep time ~14s)
    Quick = 4,
    /// Try 6 times (waits 2s, 4s, 8s, 8s, 8s; max total sleep time ~30s)
    #[default]
    Balanced = 6,
    /// Try 10 times (waits 2s, 4s, 8s, 8s, 8s, 8s, 8s, 8s, 8s; max total sleep time ~62s)
    Persistent = 10,
}

impl RetryStrategy {
    pub fn attempts(&self) -> usize {
        *self as usize
    }

    pub fn backoff(&self) -> Backoff {
        let min_wait = Duration::from_secs(2);
        let max_wait = Some(Duration::from_secs(8));
        Backoff::new(self.attempts() as u32, min_wait, max_wait)
    }
}

/// The strategy to adopt when puting and getting data from the network
///
/// Puts are followed by a verification using get, to ensure the data is stored correctly. This verification can be configured separately from the regular gets.
#[derive(Debug, Clone)]
pub struct Strategy {
    /// The number of responses to wait for before considering the put operation successful
    pub put_quorum: Quorum,
    /// The retry strategy to use if we fail to store a piece of data
    pub put_retry: RetryStrategy,
    /// The number of responses to wait for before considering the verification to be successful
    pub verification_quorum: Quorum,
    /// The number of responses to wait for before considering the get operation successful
    pub get_quorum: Quorum,
    /// The retry strategy to use if the get operation fails
    pub get_retry: RetryStrategy,
}
