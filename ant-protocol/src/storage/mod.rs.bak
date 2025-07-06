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

pub use self::{
    address::AddressParseError,
    address::{ChunkAddress, GraphEntryAddress, PointerAddress, ScratchpadAddress},
    chunks::Chunk,
    graph::{GraphContent, GraphEntry},
    header::{
        try_deserialize_record, try_serialize_record, DataTypes, RecordHeader, RecordKind,
        ValidationType,
    },
    pointer::{Pointer, PointerTarget},
    scratchpad::Scratchpad,
};
