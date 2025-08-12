// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use super::PutError;
use crate::client::NetworkAddress;
use crate::client::data_types::chunk::ChunkAddress;
use crate::client::payment::Receipt;

use serde::{Deserialize, Serialize};
use std::fmt;
use std::fmt::Display;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChunkBatchUploadState {
    pub successful: Vec<ChunkAddress>,
    pub failed: Vec<(ChunkAddress, String)>,
    pub payment: Option<Receipt>,
}

impl Display for ChunkBatchUploadState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let successes = self.successful.len();
        let failures = self.failed.len();
        let total = successes + failures;
        writeln!(f, "{failures}/{total} uploads failed")?;

        // print 3 first errors
        for (addr, err) in self.failed.iter().take(3) {
            writeln!(f, "{addr:?}: {err}")?;
        }
        if self.failed.len() > 3 {
            writeln!(f, "and {} more...", self.failed.len() - 3)?;
        }
        Ok(())
    }
}

impl ChunkBatchUploadState {
    pub fn push_error(&mut self, address: ChunkAddress, err: PutError) {
        match err {
            PutError::Network {
                address,
                network_error,
                payment,
            } => {
                let chunk_addr = match *address {
                    NetworkAddress::ChunkAddress(chunk_addr) => chunk_addr,
                    _ => {
                        error!("Skip unexpected non-chunk address: {address:?}");
                        return;
                    }
                };
                self.failed.push((chunk_addr, network_error.to_string()));
                match self.payment {
                    Some(ref mut ours) => {
                        ours.extend(payment.unwrap_or_default());
                    }
                    None => {
                        self.payment = payment;
                    }
                }
            }
            err => {
                self.failed.push((address, err.to_string()));
            }
        }
    }
}
