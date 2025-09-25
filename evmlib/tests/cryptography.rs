mod common;

use evmlib::cryptography::{hash, sign_message};

/// Test for the hash function with known inputs and outputs
#[test]
fn test_hash_function() {
    // Test empty input
    let empty_hash = hash([]);
    assert_eq!(
        format!("{empty_hash:x}"),
        "c5d2460186f7233c927e7db2dcc703c0e500b653ca82273b7bfad8045d85a470"
    );

    // Test with a known string
    let test_str = "The quick brown fox jumps over the lazy dog";
    let test_hash = hash(test_str);
    assert_eq!(
        format!("{test_hash:x}"),
        "4d741b6f1eb29cb2a9b9911c82f56fa8d73b04959d3d9d222895df6c0b28aa15"
    );

    // Test with a multi-byte input
    let multi_byte = vec![0u8, 1u8, 2u8, 3u8, 4u8];
    let multi_byte_hash = hash(multi_byte);
    assert_eq!(
        format!("{multi_byte_hash:x}"),
        "b76772ee47306482c3e219e9034bcf3f79a9bc88d6317735cd5a0e21d661acf6"
    );
}

/// Test for sign_message with invalid inputs
#[test]
fn test_sign_message_invalid_inputs() {
    // Test with invalid key
    let invalid_key = "not_a_valid_key";
    let message = b"Test message";

    let result = sign_message(invalid_key, message);
    assert!(result.is_err(), "Should fail with invalid key");

    if let Err(err) = result {
        assert!(err.to_string().contains("Failed to parse EVM secret key"));
    }
}

/// Test for sign_message functionality with a valid key
#[test]
fn test_sign_message_valid() {
    // Use a hardcoded private key for testing
    // This is a testing-only key, never use in production
    let private_key = "1111111111111111111111111111111111111111111111111111111111111111";
    let message = b"Test message for signing";

    // Sign the message
    let signature = sign_message(private_key, message);
    assert!(signature.is_ok(), "Signing should succeed with valid key");

    let signature = signature.unwrap();
    // Verify the signature is not empty and has the correct length
    assert!(!signature.is_empty());
    assert_eq!(signature.len(), 64); // ECDSA signature should be 64 bytes
}

/// Test for reproducible signatures with the same key and message
#[test]
fn test_sign_message_reproducibility() {
    // Use a hardcoded private key for testing
    // This is a testing-only key, never use in production
    let private_key = "1111111111111111111111111111111111111111111111111111111111111111";
    let message = b"Test message";

    // Sign the message twice
    let signature1 = sign_message(private_key, message).expect("Signing should succeed");
    let signature2 = sign_message(private_key, message).expect("Signing should succeed");

    // Signatures should be identical for the same key and message
    assert_eq!(signature1, signature2);
}
