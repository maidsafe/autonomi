// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

pub mod data;
pub mod files;
pub mod vault;

/// Registers are a mutable piece of data on the Network.
/// They can be read by anyone and updated only by the register owner.
/// Each entry is signed by the owner and all value history is kept on the Network.
/// They can be accessed on the Network using the RegisterAddress which is effectively the hash of the owner's [`crate::PublicKey`].
/// This means there can only be one Register per key.
///
/// The underlying structure of registers is a graph, where each version is a new [`crate::GraphEntry`]
/// Each entry is linked to the previous entry and to the next entry, like a doubly linked list
/// For fast access to the current register value, a [`crate::Pointer`] to the last entry always keeps track of the latest version
///
/// ```ignore
/// chain of GraphEntry: [register root] <-> [value2] <-> [value3] <-> [latest value]
///                                                                      ^
///                                                                      |
/// a Pointer to the latest version:                      [pointer to head]
/// ```
pub mod register;
