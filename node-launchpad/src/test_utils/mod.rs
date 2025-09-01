// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

pub mod journey;
pub mod keyboard;
pub mod mock_registry;
pub mod test_helpers;

pub use journey::*;
pub use keyboard::*;
pub use mock_registry::*;
pub use test_helpers::*;

pub const TEST_WALLET_ADDRESS: &str = "0x03b770d9cd32077cc0bf330c13c114a87643b124";
pub const TEST_STORAGE_DRIVE: &str = "Macintosh HD";
pub const TEST_LAUNCHPAD_VERSION: &str = env!("CARGO_PKG_VERSION");
