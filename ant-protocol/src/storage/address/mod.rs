pub mod chunk;
pub mod graph;
pub mod pointer_address;
pub mod scratchpad;

pub use chunk::ChunkAddress;
pub use graph::GraphEntryAddress;
pub use pointer_address::PointerAddress;
pub use scratchpad::ScratchpadAddress;

#[derive(Debug, thiserror::Error)]
pub enum AddressParseError {
    #[error("Invalid hex string: {0}")]
    Hex(#[from] hex::FromHexError),
    #[error("Invalid public key: {0}")]
    PublicKey(#[from] bls::Error),
    #[error("Invalid string length")]
    InvalidLength,
}
