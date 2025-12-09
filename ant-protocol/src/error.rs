// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::{NetworkAddress, PrettyPrintRecordKey};
use libp2p::kad::store;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// A specialised `Result` type for protocol crate.
pub type Result<T> = std::result::Result<T, Error>;

/// Main error types for the SAFE protocol.
//
// IMPORTANT DEV NOTE: when adding new variants to our Protocol Error enum,
// make sure to keep them simple variants and not complex ones to keep retro compatibility.
// this is a simple variant:  OK
//    `NewErrorVariant`
// this is a complex variant: NOT OK
//    `NewErrorVariant( some other data type )`
//
// This test test_error_retro_compatibility_complex_types demonstrates the issue with complex types.
//
#[derive(Error, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Error {
    // ---------- Chunk Proof errors
    #[error("Chunk does not exist {0:?}")]
    ChunkDoesNotExist(NetworkAddress),

    // ---------- Scratchpad errors
    /// The provided SecretyKey failed to decrypt the data
    #[error("Failed to derive CipherText from encrypted_data")]
    ScratchpadCipherTextFailed,
    /// The provided cypher text is invalid
    #[error("Provided cypher text is invalid")]
    ScratchpadCipherTextInvalid,

    // ---------- payment errors
    #[error("There was an error getting the storecost from kademlia store")]
    GetStoreQuoteFailed,
    #[error("There was an error generating the payment quote")]
    QuoteGenerationFailed,

    // ---------- replication errors
    /// Replication not found.
    #[error("Peer {holder:?} cannot find Record {key:?}")]
    ReplicatedRecordNotFound {
        /// Holder that being contacted
        holder: Box<NetworkAddress>,
        /// Key of the missing record
        key: Box<NetworkAddress>,
    },

    // ---------- record errors
    // Could not Serialize/Deserialize RecordHeader from Record
    #[error("Could not Serialize/Deserialize RecordHeader to/from Record")]
    RecordHeaderParsingFailed,
    // Could not Serialize/Deserialize Record
    #[error("Could not Serialize/Deserialize Record")]
    RecordParsingFailed,
    // The record already exists at this node
    #[error("The record already exists, so do not charge for it: {0:?}")]
    RecordExists(PrettyPrintRecordKey<'static>),

    // ---------- Record Put errors
    #[error("Error handling record put: {0}")]
    PutRecordFailed(String),
    #[error("Outdated record: with counter {counter}, expected any above {expected}")]
    OutdatedRecordCounter { counter: u64, expected: u64 },

    // ---------- Merkle payment errors
    #[error("There was an error getting the Merkle candidate quote: {0}")]
    GetMerkleCandidateQuoteFailed(String),
    #[error("Failed to sign Merkle candidate node: {0}")]
    FailedToSignMerkleCandidate(String),
    #[error("Merkle payment verification failed: {0}")]
    MerklePaymentVerificationFailed(String),
    #[error(
        "Topology verification failed: only {valid_count}/{total_paid} paid nodes in closest {closest_count}"
    )]
    TopologyVerificationFailed {
        /// Target address for distance calculations (reward pool midpoint)
        target_address: Box<NetworkAddress>,
        /// Number of paid nodes that were in the node's closest peers
        valid_count: usize,
        /// Total number of nodes that were paid
        total_paid: usize,
        /// Number of closest peers the node has
        closest_count: usize,
        /// The node's view of closest peers to the target
        #[serde(with = "crate::peer_id_serde")]
        node_peers: Vec<libp2p::PeerId>,
        /// The peers that were paid (client's view)
        #[serde(with = "crate::peer_id_serde")]
        paid_peers: Vec<libp2p::PeerId>,
    },

    // Dev Note: add new variants above this one for backward compatibility with older protocol versions
    // ---------- Unknown/fallback variant for retro compatibility
    /// Unknown error variant (for backward compatibility with newer protocol versions)
    #[error("Unknown error: the peer and you are using different protocol versions")]
    #[serde(other)]
    Unknown,
}

impl From<Error> for store::Error {
    fn from(_err: Error) -> Self {
        store::Error::ValueTooLarge
    }
}

impl From<store::Error> for Error {
    fn from(_err: store::Error) -> Self {
        Error::RecordParsingFailed
    }
}

#[cfg(test)]
mod tests {
    use libp2p::PeerId;

    use super::*;

    #[test]
    fn test_error_retro_compatibility() {
        // Test with a new struct that has a new variant
        #[derive(Error, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
        #[non_exhaustive]
        enum ExtendedError {
            #[error("Chunk does not exist {0:?}")]
            ChunkDoesNotExist(NetworkAddress),
            #[error("Failed to deserialize hex ScratchpadAddress")]
            ScratchpadHexDeserializeFailed,
            #[error("Failed to derive CipherText from encrypted_data")]
            ScratchpadCipherTextFailed,
            #[error("Provided cypher text is invalid")]
            ScratchpadCipherTextInvalid,
            #[error("There was an error getting the storecost from kademlia store")]
            GetStoreQuoteFailed,
            #[error("There was an error generating the payment quote")]
            QuoteGenerationFailed,
            #[error("Peer {holder:?} cannot find Record {key:?}")]
            ReplicatedRecordNotFound {
                holder: Box<NetworkAddress>,
                key: Box<NetworkAddress>,
            },
            #[error("Could not Serialize/Deserialize RecordHeader to/from Record")]
            RecordHeaderParsingFailed,
            #[error("Could not Serialize/Deserialize Record")]
            RecordParsingFailed,
            #[error("The record already exists, so do not charge for it: {0:?}")]
            RecordExists(PrettyPrintRecordKey<'static>),
            // New variant that doesn't exist in the original Error enum
            #[error("New error variant for testing")]
            NewErrorVariant,
            #[error("Unknown error variant")]
            #[serde(other)]
            Unknown,
        }

        // Test serialization and deserialization of ExtendedError
        let extended_error = ExtendedError::NewErrorVariant;
        let serialized = rmp_serde::to_vec(&extended_error).unwrap();

        // Test that we can deserialize into the current Error enum
        let deserialized: Error = rmp_serde::from_slice(&serialized).unwrap();
        assert_eq!(deserialized, Error::Unknown);
    }

    #[test]
    fn test_error_retro_compatibility_reduced() {
        // Test with a struct that has a missing variant (simulating older version)
        #[derive(Error, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
        #[non_exhaustive]
        enum ReducedError {
            #[error("Chunk does not exist {0:?}")]
            ChunkDoesNotExist(NetworkAddress),
            #[error("Failed to deserialize hex ScratchpadAddress")]
            ScratchpadHexDeserializeFailed,
            // removed this variant
            // - #[error("Failed to derive CipherText from encrypted_data")]
            // - ScratchpadCipherTextFailed,
            #[error("Provided cypher text is invalid")]
            ScratchpadCipherTextInvalid,
            #[error("There was an error getting the storecost from kademlia store")]
            GetStoreQuoteFailed,
            #[error("There was an error generating the payment quote")]
            QuoteGenerationFailed,
            #[error("Peer {holder:?} cannot find Record {key:?}")]
            ReplicatedRecordNotFound {
                holder: Box<NetworkAddress>,
                key: Box<NetworkAddress>,
            },
            #[error("Could not Serialize/Deserialize RecordHeader to/from Record")]
            RecordHeaderParsingFailed,
            #[error("Could not Serialize/Deserialize Record")]
            RecordParsingFailed,
            #[error("The record already exists, so do not charge for it: {0:?}")]
            RecordExists(PrettyPrintRecordKey<'static>),
            // Missing some variants that exist in the current Error enum
            #[error("Unknown error variant")]
            #[serde(other)]
            Unknown,
        }

        // Test serialization and deserialization of current Error
        let current_error = Error::ScratchpadCipherTextInvalid;
        let serialized_current = rmp_serde::to_vec(&current_error).unwrap();

        // Test that we can deserialize into ReducedError (older version)
        let deserialized_reduced: ReducedError =
            rmp_serde::from_slice(&serialized_current).unwrap();
        assert_eq!(
            deserialized_reduced,
            ReducedError::ScratchpadCipherTextInvalid
        );

        // Test that unknown variants fall back to Unknown
        let unknown_variant = Error::ScratchpadCipherTextFailed;
        let serialized_unknown = rmp_serde::to_vec(&unknown_variant).unwrap();
        let deserialized_unknown: ReducedError =
            rmp_serde::from_slice(&serialized_unknown).unwrap();
        assert_eq!(deserialized_unknown, ReducedError::Unknown);

        // Test reverse direction: ReducedError -> Error for simple variants
        let reduced_simple_error = ReducedError::GetStoreQuoteFailed;
        let serialized_reduced_simple = rmp_serde::to_vec(&reduced_simple_error).unwrap();
        let deserialized_reduced_simple: Error =
            rmp_serde::from_slice(&serialized_reduced_simple).unwrap();
        assert_eq!(deserialized_reduced_simple, Error::GetStoreQuoteFailed);

        // Test reverse direction: ReducedError -> Error for complex variants
        let addr = NetworkAddress::from(PeerId::random());
        let reduced_complex_error = ReducedError::ChunkDoesNotExist(addr.clone());
        let serialized_reduced_complex = rmp_serde::to_vec(&reduced_complex_error).unwrap();
        let deserialized_reduced_complex: Error =
            rmp_serde::from_slice(&serialized_reduced_complex).unwrap();
        assert_eq!(deserialized_reduced_complex, Error::ChunkDoesNotExist(addr));

        // Test reverse direction: ReducedError -> Error for complex struct variants
        let holder = NetworkAddress::from(PeerId::random());
        let key = NetworkAddress::from(PeerId::random());
        let reduced_struct_error = ReducedError::ReplicatedRecordNotFound {
            holder: Box::new(holder.clone()),
            key: Box::new(key.clone()),
        };
        let serialized_reduced_struct = rmp_serde::to_vec(&reduced_struct_error).unwrap();
        let deserialized_reduced_struct: Error =
            rmp_serde::from_slice(&serialized_reduced_struct).unwrap();
        assert_eq!(
            deserialized_reduced_struct,
            Error::ReplicatedRecordNotFound {
                holder: Box::new(holder),
                key: Box::new(key),
            }
        );
    }

    #[test]
    fn test_error_retro_compatibility_many_missing() {
        // test with many missing variants
        #[derive(Error, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
        #[non_exhaustive]
        enum ManyMissingVariants {
            #[error("Chunk does not exist {0:?}")]
            ChunkDoesNotExist(NetworkAddress),
            #[error("There was an error getting the storecost from kademlia store")]
            GetStoreQuoteFailed,
            // Only keeping ChunkDoesNotExist and GetStoreQuoteFailed, removing all others
            #[error("Unknown error variant")]
            #[serde(other)]
            Unknown,
        }

        // Test with ManyMissingVariants
        let current_error = Error::ScratchpadCipherTextInvalid;
        let serialized_current = rmp_serde::to_vec(&current_error).unwrap();
        let deserialized_many_missing: ManyMissingVariants =
            rmp_serde::from_slice(&serialized_current).unwrap();
        assert_eq!(deserialized_many_missing, ManyMissingVariants::Unknown);

        // Test that GetStoreQuoteFailed works with ManyMissingVariants
        let chunk_error = Error::GetStoreQuoteFailed;
        let serialized_chunk = rmp_serde::to_vec(&chunk_error).unwrap();
        let deserialized_chunk: ManyMissingVariants =
            rmp_serde::from_slice(&serialized_chunk).unwrap();
        assert_eq!(deserialized_chunk, ManyMissingVariants::GetStoreQuoteFailed);

        // Test the reverse direction: ManyMissingVariants::GetStoreQuoteFailed can be parsed as Error::GetStoreQuoteFailed
        let many_missing_error = ManyMissingVariants::GetStoreQuoteFailed;
        let serialized_many_missing = rmp_serde::to_vec(&many_missing_error).unwrap();
        let deserialized_to_error: Error = rmp_serde::from_slice(&serialized_many_missing).unwrap();
        assert_eq!(deserialized_to_error, Error::GetStoreQuoteFailed);

        // Test bidirectional compatibility for complex variant ChunkDoesNotExist
        let addr = NetworkAddress::from(PeerId::random());

        // Test Error::ChunkDoesNotExist -> ManyMissingVariants::ChunkDoesNotExist
        let chunk_error = Error::ChunkDoesNotExist(addr.clone());
        let serialized_chunk = rmp_serde::to_vec(&chunk_error).unwrap();
        let deserialized_chunk: ManyMissingVariants =
            rmp_serde::from_slice(&serialized_chunk).unwrap();
        assert_eq!(
            deserialized_chunk,
            ManyMissingVariants::ChunkDoesNotExist(addr.clone())
        );

        // Test ManyMissingVariants::ChunkDoesNotExist -> Error::ChunkDoesNotExist
        let many_missing_chunk = ManyMissingVariants::ChunkDoesNotExist(addr.clone());
        let serialized_many_missing_chunk = rmp_serde::to_vec(&many_missing_chunk).unwrap();
        let deserialized_many_missing_chunk: Error =
            rmp_serde::from_slice(&serialized_many_missing_chunk).unwrap();
        assert_eq!(
            deserialized_many_missing_chunk,
            Error::ChunkDoesNotExist(addr)
        );

        // Test that other simple variants fall back to Unknown in ManyMissingVariants
        let record_error = Error::RecordParsingFailed;
        let serialized_record = rmp_serde::to_vec(&record_error).unwrap();
        let deserialized_record: ManyMissingVariants =
            rmp_serde::from_slice(&serialized_record).unwrap();
        assert_eq!(deserialized_record, ManyMissingVariants::Unknown);
    }

    // ignore this test proves complex types retro compatibility is not supported yet
    #[test]
    fn test_error_retro_compatibility_complex_types() {
        // test with complex types
        #[derive(Error, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
        #[non_exhaustive]
        enum ComplexTypesRemoved {
            #[error("Chunk does not exist {0:?}")]
            ChunkDoesNotExist(NetworkAddress),
            #[error("Failed to deserialize hex ScratchpadAddress")]
            ScratchpadHexDeserializeFailed,
            #[error("Failed to derive CipherText from encrypted_data")]
            ScratchpadCipherTextFailed,
            #[error("Provided cypher text is invalid")]
            ScratchpadCipherTextInvalid,
            #[error("There was an error getting the storecost from kademlia store")]
            GetStoreQuoteFailed,
            #[error("There was an error generating the payment quote")]
            QuoteGenerationFailed,
            //removed this variant
            // - #[error("Peer {holder:?} cannot find Record {key:?}")]
            // - ReplicatedRecordNotFound {
            // -     holder: Box<NetworkAddress>,
            // -     key: Box<NetworkAddress>,
            // - },
            #[error("Could not Serialize/Deserialize RecordHeader to/from Record")]
            RecordHeaderParsingFailed,
            #[error("Could not Serialize/Deserialize Record")]
            RecordParsingFailed,
            #[error("The record already exists, so do not charge for it: {0:?}")]
            RecordExists(PrettyPrintRecordKey<'static>),
            // Missing some variants that exist in the current Error enum
            #[error("Unknown error variant")]
            #[serde(other)]
            Unknown,
        }

        // Test that complex types (ReplicatedRecordNotFound) fall back to Unknown
        // when the variant is missing from the older version
        let holder = NetworkAddress::from(PeerId::random());
        let key = NetworkAddress::from(PeerId::random());
        let complex_error = Error::ReplicatedRecordNotFound {
            holder: Box::new(holder),
            key: Box::new(key),
        };
        let serialized_complex = rmp_serde::to_vec(&complex_error).unwrap();

        // // Below is what we would want to do, but it's not supported yet
        // let deserialized_complex: ComplexTypesRemoved =
        //     rmp_serde::from_slice(&serialized_complex).unwrap();
        // assert_eq!(deserialized_complex, ComplexTypesRemoved::Unknown);
        // // for now it's an error
        assert!(rmp_serde::from_slice::<ComplexTypesRemoved>(&serialized_complex).is_err());

        // Test that simple variants that exist in both work correctly
        let simple_error = Error::ScratchpadCipherTextInvalid;
        let serialized_simple = rmp_serde::to_vec(&simple_error).unwrap();
        let deserialized_simple: ComplexTypesRemoved =
            rmp_serde::from_slice(&serialized_simple).unwrap();
        assert_eq!(
            deserialized_simple,
            ComplexTypesRemoved::ScratchpadCipherTextInvalid
        );

        // Test that other simple variants also work
        let addr = NetworkAddress::from(PeerId::random());
        let chunk_error = Error::ChunkDoesNotExist(addr.clone());
        let serialized_chunk = rmp_serde::to_vec(&chunk_error).unwrap();
        let deserialized_chunk: ComplexTypesRemoved =
            rmp_serde::from_slice(&serialized_chunk).unwrap();
        assert_eq!(
            deserialized_chunk,
            ComplexTypesRemoved::ChunkDoesNotExist(addr)
        );
    }
}
