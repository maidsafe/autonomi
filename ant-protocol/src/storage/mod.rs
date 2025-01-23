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
pub mod pointer;
pub use pointer::{Pointer, PointerTarget};
mod scratchpad;

pub use self::{
    address::{ChunkAddress, GraphEntryAddress, PointerAddress, ScratchpadAddress},
    chunks::Chunk,
    graph::GraphEntry,
    header::{
        try_deserialize_record, try_serialize_record, DataTypes, RecordHeader, RecordKind,
        ValidationType,
    },
    scratchpad::Scratchpad,
};
