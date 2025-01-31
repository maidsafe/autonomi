// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use ant_networking::time::{sleep, Duration, Instant};

pub struct RateLimiter {
    last_request_time: Option<Instant>,
}

impl RateLimiter {
    pub fn new() -> Self {
        Self {
            last_request_time: None,
        }
    }

    pub async fn wait_interval_since_last_request(&mut self, interval_in_ms: u64) {
        if let Some(last_request_time) = self.last_request_time {
            let elapsed_time = last_request_time.elapsed();

            let interval = Duration::from_millis(interval_in_ms);

            if elapsed_time < interval {
                sleep(interval - elapsed_time).await;
            }
        }

        self.last_request_time = Some(Instant::now());
    }
}
