// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

pub mod merkle_payment_vault;
pub mod network_token;
pub mod payment_vault;

pub fn data_type_conversion(data_type: u32) -> u8 {
    match data_type {
        0 => 2, // Chunk
        1 => 0, // GraphEntry
        2 => 3, // Pointer
        3 => 1, // Scratchpad
        _ => 4, // Does not exist
    }
}
