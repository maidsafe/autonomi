// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

pub mod evm;
pub mod testnet;

use bytes::Bytes;
use rand::Rng;

/// Generate random data of the given length.
pub fn gen_random_data(len: usize) -> Bytes {
    let mut data = vec![0u8; len];
    rand::thread_rng().fill(&mut data[..]);
    Bytes::from(data)
}
