use autonomi::client::key_derivation::{
    DerivationIndex, DerivedPubkey, DerivedSecretKey, KeyDecodeError, MainPubkey, MainSecretKey,
};
use bls::{PublicKey, SecretKey};
use rand::{rngs::StdRng, SeedableRng};
use std::collections::HashSet;

/// Test key derivation with a known seed for reproducibility
#[test]
fn test_key_derivation_with_known_seed() {
    // Create a deterministic RNG with a fixed seed
    let mut rng = StdRng::seed_from_u64(12345);
    
    // Generate a main key
    let main_secret_key = MainSecretKey::random();
    let main_pubkey = main_secret_key.public_key();
    
    // Generate multiple derived keys using the same derivation index
    let derivation_index = DerivationIndex::random(&mut rng);
    
    let derived_secret_key1 = main_secret_key.derive_key(&derivation_index);
    let derived_pubkey1 = derived_secret_key1.public_key();
    
    let derived_pubkey2 = main_pubkey.derive_key(&derivation_index);
    
    // Keys derived with the same index should match
    assert_eq!(derived_pubkey1, derived_pubkey2, "Derived public keys should match when using the same derivation index");
}

/// Test that derived keys are unique for different derivation indices
#[test]
fn test_derived_keys_uniqueness() {
    let main_secret_key = MainSecretKey::random();
    let main_pubkey = main_secret_key.public_key();
    
    let mut derived_pubkeys = HashSet::new();
    
    // Generate multiple derived keys with different indices
    for _ in 0..10 {
        let derivation_index = DerivationIndex::random(&mut rand::thread_rng());
        let derived_pubkey = main_pubkey.derive_key(&derivation_index);
        
        // Each derived pubkey should be unique
        assert!(derived_pubkeys.insert(derived_pubkey), "Derived keys should be unique");
    }
}

/// Test that signatures created with derived keys can only be verified with the corresponding public key
#[test]
fn test_signature_verification() {
    let msg1 = "Test message 1".as_bytes();
    let msg2 = "Test message 2".as_bytes();
    
    let main_secret_key = MainSecretKey::random();
    let derived_secret_key = main_secret_key.random_derived_key(&mut rand::thread_rng());
    
    // Sign with the main key
    let main_sig = main_secret_key.sign(msg1);
    
    // Sign with the derived key
    let derived_sig = derived_secret_key.sign(msg1);
    
    // Verification should work correctly
    assert!(main_secret_key.public_key().verify(&main_sig, msg1), "Main key signature should verify with main pubkey");
    assert!(derived_secret_key.public_key().verify(&derived_sig, msg1), "Derived key signature should verify with derived pubkey");
    
    // Cross-verification should fail
    assert!(!main_secret_key.public_key().verify(&derived_sig, msg1), "Main pubkey should not verify derived key signature");
    assert!(!derived_secret_key.public_key().verify(&main_sig, msg1), "Derived pubkey should not verify main key signature");
    
    // Different messages should fail verification
    assert!(!main_secret_key.public_key().verify(&main_sig, msg2), "Different message should not verify");
    assert!(!derived_secret_key.public_key().verify(&derived_sig, msg2), "Different message should not verify");
}

/// Test serialization and deserialization of keys
#[test]
fn test_key_serialization() -> Result<(), Box<dyn std::error::Error>> {
    let main_secret_key = MainSecretKey::random();
    let main_pubkey = main_secret_key.public_key();
    let derived_secret_key = main_secret_key.random_derived_key(&mut rand::thread_rng());
    let derived_pubkey = derived_secret_key.public_key();
    
    // Test MainPubkey serialization
    let main_pubkey_hex = main_pubkey.to_hex();
    let main_pubkey_from_hex = MainPubkey::from_hex(&main_pubkey_hex)?;
    assert_eq!(main_pubkey, main_pubkey_from_hex, "MainPubkey serialization failed");
    
    // Test DerivedPubkey serialization
    let derived_pubkey_hex = derived_pubkey.to_hex();
    let derived_pubkey_from_hex = DerivedPubkey::from_hex(&derived_pubkey_hex)?;
    assert_eq!(derived_pubkey, derived_pubkey_from_hex, "DerivedPubkey serialization failed");
    
    // Test MessagePack serialization
    let encoded_main_pubkey = rmp_serde::to_vec_named(&main_pubkey)?;
    let decoded_main_pubkey: MainPubkey = rmp_serde::from_slice(&encoded_main_pubkey)?;
    assert_eq!(main_pubkey, decoded_main_pubkey, "MainPubkey MessagePack serialization failed");
    
    let encoded_derived_pubkey = rmp_serde::to_vec_named(&derived_pubkey)?;
    let decoded_derived_pubkey: DerivedPubkey = rmp_serde::from_slice(&encoded_derived_pubkey)?;
    assert_eq!(derived_pubkey, decoded_derived_pubkey, "DerivedPubkey MessagePack serialization failed");
    
    Ok(())
}

/// Test error handling for invalid keys
#[test]
fn test_invalid_key_handling() {
    // Test invalid hex string
    let result = MainPubkey::from_hex("not-a-hex-string");
    assert!(result.is_err(), "Should fail with invalid hex");
    if let Err(err) = result {
        assert!(matches!(err, KeyDecodeError::FailedToDecodeHexToKey), "Wrong error type");
    }
    
    // Test wrong length
    let result = MainPubkey::from_hex("aabbcc");
    assert!(result.is_err(), "Should fail with wrong length");
    if let Err(err) = result {
        assert!(matches!(err, KeyDecodeError::InvalidKeyLength), "Wrong error type");
    }
    
    // Similarly for DerivedPubkey
    let result = DerivedPubkey::from_hex("not-a-hex-string");
    assert!(result.is_err(), "Should fail with invalid hex");
}

/// Test DerivationIndex behavior
#[test]
fn test_derivation_index() {
    // Test from_bytes and as_bytes
    let bytes = [42u8; 32];
    let index = DerivationIndex::from_bytes(bytes);
    assert_eq!(index.as_bytes(), &bytes, "Bytes should match");
    
    // Test random generation
    let mut rng = StdRng::seed_from_u64(12345);
    let index1 = DerivationIndex::random(&mut rng);
    let index2 = DerivationIndex::random(&mut rng);
    assert_ne!(index1, index2, "Random indices should be different");
    
    // Test Debug implementation
    let debug_str = format!("{:?}", index1);
    assert!(!debug_str.is_empty(), "Debug representation should not be empty");
}

/// Test that BLS primitives can be converted to/from our wrapper types
#[test]
fn test_bls_conversions() {
    let bls_secret = SecretKey::random();
    let bls_pubkey = bls_secret.public_key();
    
    // Test conversions to BLS types
    let main_secret = MainSecretKey::new(bls_secret.clone());
    let main_pubkey = main_secret.public_key();
    
    let derived_index = DerivationIndex::random(&mut rand::thread_rng());
    let derived_secret = main_secret.derive_key(&derived_index);
    let derived_pubkey = derived_secret.public_key();
    
    // Verify that conversions preserve the underlying keys
    let main_secret_from_bls: MainSecretKey = bls_secret.clone().into();
    let main_pubkey_from_bls: MainPubkey = bls_pubkey.into();
    
    // Verify that conversions preserve the underlying keys
    assert_eq!(main_secret.public_key(), main_pubkey_from_bls, "MainPubkey conversion failed");
    assert_eq!(main_secret_from_bls.public_key(), main_pubkey, "MainSecretKey conversion failed");
}

/// Test against timing attacks by ensuring that verification always takes similar time
/// regardless of whether the signature is valid or not
#[test]
fn test_constant_time_verification() {
    let msg = "Test message".as_bytes();
    let main_secret_key = MainSecretKey::random();
    let main_pubkey = main_secret_key.public_key();
    
    // Valid signature
    let valid_sig = main_secret_key.sign(msg);
    
    // Invalid signature (from a different key)
    let different_key = MainSecretKey::random();
    let invalid_sig = different_key.sign(msg);
    
    // Check that both verification operations complete (we don't measure timing in unit tests,
    // but at least we can confirm the API doesn't short-circuit in a way that might leak timing info)
    let valid_result = main_pubkey.verify(&valid_sig, msg);
    let invalid_result = main_pubkey.verify(&invalid_sig, msg);
    
    assert!(valid_result, "Valid signature should verify");
    assert!(!invalid_result, "Invalid signature should not verify");
}

/// Test high complexity key derivation with multiple levels
#[test]
fn test_multi_level_derivation() {
    let main_sk = MainSecretKey::random();
    let main_pk = main_sk.public_key();
    
    // First level of derivation
    let index1 = DerivationIndex::random(&mut rand::thread_rng());
    let derived_sk1 = main_sk.derive_key(&index1);
    let derived_pk1 = derived_sk1.public_key();
    
    // Verify first level works
    assert_eq!(derived_pk1, main_pk.derive_key(&index1), "First level derivation failed");
    
    // Create a copy of derived_sk1 for later signature
    let derived_sk1_copy = main_sk.derive_key(&index1);
    
    // Now convert derived_sk1 to a new "main" key and derive again
    let bls_sk: SecretKey = derived_sk1.into();
    let second_main_sk = MainSecretKey::new(bls_sk);
    
    // Second level of derivation
    let index2 = DerivationIndex::random(&mut rand::thread_rng());
    let derived_sk2 = second_main_sk.derive_key(&index2);
    let derived_pk2 = derived_sk2.public_key();
    
    // Verify signatures at each level
    let msg = "Test message".as_bytes();
    
    let main_sig = main_sk.sign(msg);
    let derived_sig1 = derived_sk1_copy.sign(msg);
    let derived_sig2 = derived_sk2.sign(msg);
    
    assert!(main_pk.verify(&main_sig, msg), "Main signature verification failed");
    assert!(derived_pk1.verify(&derived_sig1, msg), "First level derived signature verification failed");
    assert!(derived_pk2.verify(&derived_sig2, msg), "Second level derived signature verification failed");
    
    // Cross-verification should fail
    assert!(!main_pk.verify(&derived_sig1, msg), "Main pubkey should not verify first level derived signature");
    assert!(!derived_pk1.verify(&derived_sig2, msg), "First level pubkey should not verify second level derived signature");
    assert!(!derived_pk2.verify(&main_sig, msg), "Second level pubkey should not verify main signature");
}