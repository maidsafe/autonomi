// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

mod address;
mod chunks;
mod graph;
mod header;
mod pointer;
mod scratchpad;

pub use self::address::AddressParseError;
pub use self::address::ChunkAddress;
pub use self::address::GraphEntryAddress;
pub use self::address::PointerAddress;
pub use self::address::ScratchpadAddress;
pub use self::chunks::Chunk;
pub use self::graph::GraphContent;
pub use self::graph::GraphEntry;
pub use self::header::try_deserialize_record;
pub use self::header::try_serialize_record;
pub use self::header::DataTypes;
pub use self::header::RecordHeader;
pub use self::header::RecordKind;
pub use self::header::ValidationType;
pub use self::pointer::Pointer;
pub use self::pointer::PointerTarget;
pub use self::scratchpad::Scratchpad;
