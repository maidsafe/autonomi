// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use color_eyre::Result;
use color_eyre::eyre::eyre;
use rand::Rng;
use ring::aead::{BoundKey, Nonce, NonceSequence};
use ring::error::Unspecified;
use std::num::NonZeroU32;
use std::sync::LazyLock;

const SALT_LENGTH: usize = 8;
const NONCE_LENGTH: usize = 12;

/// Number of iterations for pbkdf2.
static ITERATIONS: LazyLock<NonZeroU32> = LazyLock::new(|| {
    #[allow(clippy::expect_used)] // NonZeroU32::new(100_000) is infallible
    NonZeroU32::new(100_000).expect("Infallible")
});

struct NonceSeq([u8; 12]);

impl NonceSequence for NonceSeq {
    fn advance(&mut self) -> Result<Nonce, Unspecified> {
        Nonce::try_assume_unique_for_key(&self.0)
    }
}

pub fn encrypt_private_key(private_key: &str, password: &str) -> Result<String> {
    // Generate a random salt
    // Salt is used to ensure unique derived keys even for identical passwords
    let mut salt = [0u8; SALT_LENGTH];
    rand::thread_rng().fill(&mut salt);

    // Generate a random nonce
    // Nonce is used to ensure unique encryption outputs even for identical inputs
    let mut nonce = [0u8; NONCE_LENGTH];
    rand::thread_rng().fill(&mut nonce);

    let mut key = [0; 32];

    // Derive a key from the password using PBKDF2 with HMAC<Sha512>
    // PBKDF2 is used for key derivation to mitigate brute-force attacks by making key derivation computationally expensive
    // HMAC<Sha512> is used as the pseudorandom function for its security properties
    ring::pbkdf2::derive(
        ring::pbkdf2::PBKDF2_HMAC_SHA512,
        *ITERATIONS,
        &salt,
        password.as_bytes(),
        &mut key,
    );

    // Create an unbound key using CHACHA20_POLY1305 algorithm
    // CHACHA20_POLY1305 is a fast and secure AEAD (Authenticated Encryption with Associated Data) algorithm
    let unbound_key = ring::aead::UnboundKey::new(&ring::aead::CHACHA20_POLY1305, &key)
        .map_err(|_| eyre!("Failed to encrypt key: Could not create unbound key"))?;

    // Create a sealing key with the unbound key and nonce
    let mut sealing_key = ring::aead::SealingKey::new(unbound_key, NonceSeq(nonce));
    let aad = ring::aead::Aad::from(&[]);

    // Convert the secret key to bytes
    let private_key_bytes = String::from(private_key).into_bytes();
    let mut encrypted_private_key = private_key_bytes;

    // seal_in_place_append_tag encrypts the data and appends an authentication tag to ensure data integrity
    sealing_key
        .seal_in_place_append_tag(aad, &mut encrypted_private_key)
        .map_err(|_| eyre!("Failed to encrypt key: Could not seal sealing key"))?;

    let mut encrypted_data = Vec::new();
    encrypted_data.extend_from_slice(&salt);
    encrypted_data.extend_from_slice(&nonce);
    encrypted_data.extend_from_slice(&encrypted_private_key);

    // Return the encrypted secret key along with salt and nonce encoded as hex strings
    Ok(hex::encode(encrypted_data))
}

pub fn decrypt_private_key(encrypted_data: &str, password: &str) -> Result<String> {
    let encrypted_data = hex::decode(encrypted_data)
        .map_err(|_| eyre!("Failed to decrypt key: Encrypted data is invalid"))?;

    let salt: [u8; SALT_LENGTH] = encrypted_data[..SALT_LENGTH]
        .try_into()
        .map_err(|_| eyre!("Failed to decrypt key: Could not find salt"))?;

    let nonce: [u8; NONCE_LENGTH] = encrypted_data[SALT_LENGTH..SALT_LENGTH + NONCE_LENGTH]
        .try_into()
        .map_err(|_| eyre!("Failed to decrypt key: Could not find nonce"))?;

    let encrypted_private_key = &encrypted_data[SALT_LENGTH + NONCE_LENGTH..];

    let mut key = [0; 32];

    // Reconstruct the key from salt and password
    ring::pbkdf2::derive(
        ring::pbkdf2::PBKDF2_HMAC_SHA512,
        *ITERATIONS,
        &salt,
        password.as_bytes(),
        &mut key,
    );

    // Create an unbound key from the previously reconstructed key
    let unbound_key = ring::aead::UnboundKey::new(&ring::aead::CHACHA20_POLY1305, &key)
        .map_err(|_| eyre!("Failed to decrypt key: Could not create unbound key"))?;

    // Create an opening key using the unbound key and original nonce
    let mut opening_key = ring::aead::OpeningKey::new(unbound_key, NonceSeq(nonce));
    let aad = ring::aead::Aad::from(&[]);

    let mut encrypted_private_key = encrypted_private_key.to_vec();

    // Decrypt the encrypted secret key bytes
    let decrypted_data = opening_key
        .open_in_place(aad, &mut encrypted_private_key)
        .map_err(|_| {
            eyre!("Failed to decrypt key: Could not open encrypted key, please check the password")
        })?;

    // Create secret key from decrypted byte
    String::from_utf8(decrypted_data.to_vec())
        .map_err(|e| eyre!("Failed to convert private key: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use autonomi::Wallet;

    #[test]
    fn test_encrypt_decrypt_private_key() {
        let key = Wallet::random_private_key();
        let password = "password123".to_string();

        let encrypted_key =
            encrypt_private_key(&key, &password).expect("Failed to encrypt the private key");

        let decrypted_key = decrypt_private_key(&encrypted_key, &password)
            .expect("Failed to decrypt the private key");

        assert_eq!(
            decrypted_key, key,
            "Decrypted key does not match the original private key"
        );
    }

    #[test]
    fn test_wrong_password() {
        let key = Wallet::random_private_key();
        let password = "password123".to_string();

        let encrypted_key =
            encrypt_private_key(&key, &password).expect("Failed to encrypt the private key");

        let wrong_password = "password456".to_string();
        let result = decrypt_private_key(&encrypted_key, &wrong_password);

        assert!(
            result.is_err(),
            "Decryption should not succeed with a wrong password"
        );
    }
    
    #[test]
    fn test_different_passwords_produce_different_results() -> Result<()> {
        let key = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        
        let password1 = "password1";
        let password2 = "password2";
        
        let encrypted_key1 = encrypt_private_key(key, password1)?;
        let encrypted_key2 = encrypt_private_key(key, password2)?;
        
        assert_ne!(encrypted_key1, encrypted_key2, "Different passwords should produce different encryption results");
        
        Ok(())
    }
    
    #[test]
    fn test_same_key_password_different_results() -> Result<()> {
        let key = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let password = "same_password";
        
        let encrypted_key1 = encrypt_private_key(key, password)?;
        let encrypted_key2 = encrypt_private_key(key, password)?;
        
        assert_ne!(encrypted_key1, encrypted_key2, "Same key and password should still produce different encryption results due to random salt and nonce");
        
        Ok(())
    }

    #[test]
    fn test_various_key_lengths() -> Result<()> {
        // Test with short key
        let short_key = "12345";
        let password = "password";
        
        let encrypted_short = encrypt_private_key(short_key, password)?;
        let decrypted_short = decrypt_private_key(&encrypted_short, password)?;
        assert_eq!(decrypted_short, short_key, "Short key decryption failed");
        
        // Test with a typical length key (32 bytes hex-encoded)
        let typical_key = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let encrypted_typical = encrypt_private_key(typical_key, password)?;
        let decrypted_typical = decrypt_private_key(&encrypted_typical, password)?;
        assert_eq!(decrypted_typical, typical_key, "Typical key decryption failed");
        
        // Test with a long key
        let long_key = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let encrypted_long = encrypt_private_key(long_key, password)?;
        let decrypted_long = decrypt_private_key(&encrypted_long, password)?;
        assert_eq!(decrypted_long, long_key, "Long key decryption failed");
        
        Ok(())
    }

    #[test]
    fn test_various_passwords() -> Result<()> {
        let key = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        
        // Test with empty password (should still work)
        let empty_password = "";
        let encrypted_empty = encrypt_private_key(key, empty_password)?;
        let decrypted_empty = decrypt_private_key(&encrypted_empty, empty_password)?;
        assert_eq!(decrypted_empty, key, "Empty password encryption/decryption failed");
        
        // Test with short password
        let short_password = "a";
        let encrypted_short = encrypt_private_key(key, short_password)?;
        let decrypted_short = decrypt_private_key(&encrypted_short, short_password)?;
        assert_eq!(decrypted_short, key, "Short password encryption/decryption failed");
        
        // Test with long password
        let long_password = "this_is_a_very_long_password_with_many_characters_and_should_still_work_correctly";
        let encrypted_long = encrypt_private_key(key, long_password)?;
        let decrypted_long = decrypt_private_key(&encrypted_long, long_password)?;
        assert_eq!(decrypted_long, key, "Long password encryption/decryption failed");
        
        // Test with unicode password
        let unicode_password = "пароль密码パスワード";
        let encrypted_unicode = encrypt_private_key(key, unicode_password)?;
        let decrypted_unicode = decrypt_private_key(&encrypted_unicode, unicode_password)?;
        assert_eq!(decrypted_unicode, key, "Unicode password encryption/decryption failed");
        
        Ok(())
    }

    #[test]
    fn test_encryption_includes_salt_and_nonce() -> Result<()> {
        let key = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let password = "password";
        
        let encrypted = encrypt_private_key(key, password)?;
        
        // Decode the hex
        let binary_data = hex::decode(&encrypted)?;
        
        // Verify it's long enough to contain salt (8 bytes) and nonce (12 bytes)
        assert!(binary_data.len() > 20, "Encrypted data should include salt and nonce");
        
        Ok(())
    }
}
