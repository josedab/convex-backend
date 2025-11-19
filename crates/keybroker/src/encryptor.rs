use std::io::{
    Cursor,
    Read as _,
};

use anyhow::Context;
use aws_lc_rs::{
    aead,
    kdf,
    rand::{
        SecureRandom,
        SystemRandom,
    },
};
use byteorder::ReadBytesExt;
use prost::Message;

use crate::{
    key_derivation::{
        DerivedKey,
        KeyPurpose,
        DERIVED_KEY_LEN,
    },
    Secret,
};

const AEAD_ALGORITHM: aead::Algorithm = aead::AES_128_GCM_SIV;
const KEY_LEN: usize = 16;
#[test]
fn test_key_len() {
    assert_eq!(KEY_LEN, AEAD_ALGORITHM.key_len());
}

#[derive(Clone)]
pub struct Encryptor<const DETERMINISTIC: bool> {
    derived_key: [u8; KEY_LEN],
}
pub type RandomEncryptor = Encryptor<false>;
pub type DeterministicEncryptor = Encryptor<true>;

// These are arbitrary strings; it's only important that we never reuse the
// exact same string for two different logical purposes.
pub struct Purpose<const DETERMINISTIC: bool = false>(&'static str);
pub type DeterministicPurpose = Purpose<true>;
impl Purpose {
    pub const ACTION_CALLBACK_TOKEN: Purpose = Purpose("action callback token");
    pub const ADMIN_KEY: Purpose = Purpose("admin key");
    /// Cursors are issued in UDFs and are also fed back as arguments. As such
    /// we want them to be deterministic to avoid breaking caching.
    /// These do not need to be secret in the first place - only tamper-proof.
    pub const CURSOR: DeterministicPurpose = Purpose("cursor");
    pub const QUERY_JOURNAL: Purpose = Purpose("query journal");
    pub const STORE_FILE_AUTHORIZATION: Purpose = Purpose("store file authorization");
}

impl<const DETERMINISTIC: bool> Purpose<DETERMINISTIC> {
    fn as_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }
}

const KDF_ALGORITHM: &kdf::KbkdfCtrHmacAlgorithm =
    kdf::get_kbkdf_ctr_hmac_algorithm(kdf::KbkdfCtrHmacAlgorithmId::Sha256).unwrap();

impl<const DETERMINISTIC: bool> Encryptor<DETERMINISTIC> {
    pub fn derive_from_secret(
        secret: &Secret,
        purpose: Purpose<DETERMINISTIC>,
    ) -> anyhow::Result<Self> {
        let mut derived_key = [0; KEY_LEN];
        kdf::kbkdf_ctr_hmac(
            KDF_ALGORITHM,
            secret.as_bytes(),
            purpose.as_bytes(),
            &mut derived_key,
        )
        .context("KBKDF failed")?;
        Ok(Self { derived_key })
    }

    // TODO: do not send instance secrets to funrun, only derived keys
    #[allow(unused)]
    pub fn derived_key(&self) -> [u8; KEY_LEN] {
        self.derived_key
    }

    #[allow(unused)]
    pub fn from_derived_key(derived_key: [u8; KEY_LEN]) -> Self {
        Self { derived_key }
    }

    fn key(&self) -> aead::LessSafeKey {
        aead::LessSafeKey::new(
            aead::UnboundKey::new(&AEAD_ALGORITHM, &self.derived_key)
                .expect("KEY_LEN == AEAD_ALGORITHM.key_len()"),
        )
    }

    pub fn encrypt_proto<T: Message>(&self, version: u8, message: &T) -> String {
        let mut nonce = [0; aead::NONCE_LEN];
        // N.B.: AES-GCM-SIV is "nonce-misuse-resistant". When
        // DETERMINISTIC=true we intentionally "misuse" it by using a constant
        // nonce for all messages. This does not break the encryption (unlike
        // AES-GCM) but merely leaks whether messages are identical. That is,
        // anyone can tell whether two encrypted messages correspond to the same
        // plaintext - which is exactly what we want from deterministic
        // encryption.
        if !DETERMINISTIC {
            SystemRandom::new()
                .fill(&mut nonce)
                .expect("SystemRandom failed");
        }
        let mut encoded_message = message.encode_to_vec();
        let tag = self
            .key()
            .seal_in_place_separate_tag(
                aead::Nonce::assume_unique_for_key(nonce),
                aead::Aad::from(&[version]),
                &mut encoded_message,
            )
            .expect("encryption failed");

        let mut buffer = Vec::with_capacity(
            1 + if DETERMINISTIC { 0 } else { nonce.len() }
                + encoded_message.len()
                + AEAD_ALGORITHM.tag_len(),
        );
        buffer.push(version);
        if !DETERMINISTIC {
            buffer.extend_from_slice(&nonce);
        }
        buffer.extend_from_slice(&encoded_message);
        buffer.extend_from_slice(tag.as_ref());
        hex::encode(buffer)
    }

    pub fn decrypt_proto<M: Default + Message>(
        &self,
        version: u8,
        encoded: &str,
    ) -> anyhow::Result<M> {
        let mut bytes = hex::decode(encoded)?;
        let mut reader = Cursor::new(&bytes[..]);
        let message_version = reader.read_u8()?;
        if message_version != version {
            anyhow::bail!("Invalid message version {}", message_version);
        }
        let mut nonce = [0; aead::NONCE_LEN];
        if !DETERMINISTIC {
            reader.read_exact(&mut nonce)?;
        }
        let pos = reader.position() as usize;
        let ciphertext_and_tag = &mut bytes[pos..];
        let plaintext = self
            .key()
            .open_in_place(
                aead::Nonce::assume_unique_for_key(nonce),
                aead::Aad::from(&[version]),
                ciphertext_and_tag,
            )
            .map_err(|_| anyhow::anyhow!("Failed to decrypt ciphertext"))?;
        Ok(M::decode(&*plaintext)?)
    }
}

#[test]
fn test_encryptor() {
    use common::testing::assert_contains;

    let secret = Secret::random();
    let encryptor = RandomEncryptor::derive_from_secret(&secret, Purpose("testing")).unwrap();
    let message = "very cool message".to_owned();
    let encoded = encryptor.encrypt_proto(11, &message);
    // RandomEncryptor is nondeterministic
    assert_ne!(encoded, encryptor.encrypt_proto(11, &message));
    assert_eq!(
        encryptor.decrypt_proto::<String>(11, &encoded).unwrap(),
        message
    );
    // decrypting with the wrong version should fail
    assert_contains(
        &encryptor.decrypt_proto::<String>(12, &encoded).unwrap_err(),
        "Invalid message version",
    );

    // An encryptor with a different purpose should not recognize the message
    let encryptor2 = RandomEncryptor::derive_from_secret(&secret, Purpose("testing2")).unwrap();
    assert_contains(
        &encryptor2
            .decrypt_proto::<String>(11, &encoded)
            .unwrap_err(),
        "Failed to decrypt",
    );
}

#[test]
fn test_deterministic_encryptor() {
    use common::testing::assert_contains;

    let secret = Secret::random();
    let encryptor =
        DeterministicEncryptor::derive_from_secret(&secret, Purpose("testing")).unwrap();
    let message = "very cool message".to_owned();
    let encoded = encryptor.encrypt_proto(11, &message);
    assert_eq!(encoded, encryptor.encrypt_proto(11, &message));
    assert_eq!(
        encryptor.decrypt_proto::<String>(11, &encoded).unwrap(),
        message
    );
    // decrypting with the wrong version should fail
    assert_contains(
        &encryptor.decrypt_proto::<String>(12, &encoded).unwrap_err(),
        "Invalid message version",
    );

    // An encryptor with a different purpose should not recognize the message
    let encryptor2 =
        DeterministicEncryptor::derive_from_secret(&secret, Purpose("testing2")).unwrap();
    assert_contains(
        &encryptor2
            .decrypt_proto::<String>(11, &encoded)
            .unwrap_err(),
        "Failed to decrypt",
    );
}

// =============================================================================
// HKDF-based FunrunEncryptor (RFC-0006)
// =============================================================================

/// AES-256-GCM algorithm for the new HKDF-based encryptor
const FUNRUN_AEAD_ALGORITHM: aead::Algorithm = aead::AES_256_GCM;
const FUNRUN_NONCE_LEN: usize = 12;

#[test]
fn test_funrun_key_len() {
    assert_eq!(DERIVED_KEY_LEN, FUNRUN_AEAD_ALGORITHM.key_len());
}

/// Encryptor for function runner using HKDF-derived keys
///
/// This encryptor uses:
/// - AES-256-GCM for encryption
/// - HKDF-derived keys for purpose-specific encryption
/// - Random nonces for each encryption
///
/// It follows the principle of least privilege by only having access
/// to the derived key for its specific purpose, not the full instance secret.
pub struct FunrunEncryptor {
    derived_key: DerivedKey,
}

impl FunrunEncryptor {
    /// Create a new FunrunEncryptor with a derived key
    ///
    /// # Panics
    ///
    /// Panics if the derived key's purpose is not `FunrunDataEncryption`.
    pub fn new(derived_key: DerivedKey) -> Self {
        assert_eq!(
            derived_key.purpose(),
            KeyPurpose::FunrunDataEncryption,
            "Wrong key purpose for FunrunEncryptor: expected FunrunDataEncryption, got {:?}",
            derived_key.purpose()
        );
        Self { derived_key }
    }

    /// Create a FunrunEncryptor from raw key bytes
    ///
    /// This is useful when receiving a derived key over the wire.
    pub fn from_key_bytes(key: [u8; DERIVED_KEY_LEN]) -> Self {
        Self {
            derived_key: DerivedKey::from_bytes(key, KeyPurpose::FunrunDataEncryption),
        }
    }

    /// Get the derived key bytes for serialization
    pub fn key_bytes(&self) -> &[u8; DERIVED_KEY_LEN] {
        self.derived_key.as_bytes()
    }

    fn aead_key(&self) -> aead::LessSafeKey {
        aead::LessSafeKey::new(
            aead::UnboundKey::new(&FUNRUN_AEAD_ALGORITHM, self.derived_key.as_bytes())
                .expect("DERIVED_KEY_LEN == AES_256_GCM.key_len()"),
        )
    }

    /// Encrypt plaintext data
    ///
    /// Returns ciphertext with prepended nonce: `[nonce (12 bytes)][ciphertext][tag]`
    pub fn encrypt(&self, plaintext: &[u8]) -> anyhow::Result<Vec<u8>> {
        // Generate random nonce
        let mut nonce_bytes = [0u8; FUNRUN_NONCE_LEN];
        SystemRandom::new()
            .fill(&mut nonce_bytes)
            .map_err(|_| anyhow::anyhow!("Failed to generate random nonce"))?;

        let nonce = aead::Nonce::assume_unique_for_key(nonce_bytes);

        // Encrypt
        let mut buffer = plaintext.to_vec();
        let tag = self
            .aead_key()
            .seal_in_place_separate_tag(nonce, aead::Aad::empty(), &mut buffer)
            .map_err(|_| anyhow::anyhow!("Encryption failed"))?;

        // Prepend nonce to ciphertext
        let mut result = Vec::with_capacity(FUNRUN_NONCE_LEN + buffer.len() + tag.as_ref().len());
        result.extend_from_slice(&nonce_bytes);
        result.extend_from_slice(&buffer);
        result.extend_from_slice(tag.as_ref());

        Ok(result)
    }

    /// Decrypt ciphertext data
    ///
    /// Expects format: `[nonce (12 bytes)][ciphertext][tag]`
    pub fn decrypt(&self, ciphertext: &[u8]) -> anyhow::Result<Vec<u8>> {
        if ciphertext.len() < FUNRUN_NONCE_LEN {
            anyhow::bail!("Ciphertext too short: expected at least {} bytes", FUNRUN_NONCE_LEN);
        }

        let (nonce_bytes, encrypted) = ciphertext.split_at(FUNRUN_NONCE_LEN);
        let nonce = aead::Nonce::assume_unique_for_key(
            nonce_bytes
                .try_into()
                .expect("nonce_bytes should be 12 bytes"),
        );

        let mut buffer = encrypted.to_vec();
        let plaintext = self
            .aead_key()
            .open_in_place(nonce, aead::Aad::empty(), &mut buffer)
            .map_err(|_| anyhow::anyhow!("Decryption failed"))?;

        Ok(plaintext.to_vec())
    }
}

impl Clone for FunrunEncryptor {
    fn clone(&self) -> Self {
        Self {
            derived_key: self.derived_key.clone(),
        }
    }
}

#[test]
fn test_funrun_encryptor() {
    let secret = Secret::random();
    let derived_key = DerivedKey::derive(&secret, KeyPurpose::FunrunDataEncryption);
    let encryptor = FunrunEncryptor::new(derived_key);

    let plaintext = b"test message for funrun encryption";
    let ciphertext = encryptor.encrypt(plaintext).unwrap();

    // Ciphertext should be longer than plaintext (nonce + tag)
    assert!(ciphertext.len() > plaintext.len());

    // Decrypt should recover plaintext
    let decrypted = encryptor.decrypt(&ciphertext).unwrap();
    assert_eq!(decrypted, plaintext);

    // Random encryptor produces different ciphertext each time
    let ciphertext2 = encryptor.encrypt(plaintext).unwrap();
    assert_ne!(ciphertext, ciphertext2);

    // But both decrypt to the same plaintext
    let decrypted2 = encryptor.decrypt(&ciphertext2).unwrap();
    assert_eq!(decrypted2, plaintext);
}

#[test]
fn test_funrun_encryptor_from_key_bytes() {
    let secret = Secret::random();
    let derived_key = DerivedKey::derive(&secret, KeyPurpose::FunrunDataEncryption);
    let encryptor1 = FunrunEncryptor::new(derived_key);

    // Create second encryptor from key bytes
    let encryptor2 = FunrunEncryptor::from_key_bytes(*encryptor1.key_bytes());

    // Both should be able to decrypt each other's ciphertext
    let plaintext = b"cross-encryptor test";
    let ciphertext = encryptor1.encrypt(plaintext).unwrap();
    let decrypted = encryptor2.decrypt(&ciphertext).unwrap();
    assert_eq!(decrypted, plaintext);
}

#[test]
#[should_panic(expected = "Wrong key purpose")]
fn test_funrun_encryptor_wrong_purpose() {
    let secret = Secret::random();
    let derived_key = DerivedKey::derive(&secret, KeyPurpose::TokenSigning);
    FunrunEncryptor::new(derived_key); // Should panic
}

#[test]
fn test_funrun_encryptor_decrypt_invalid_ciphertext() {
    let secret = Secret::random();
    let derived_key = DerivedKey::derive(&secret, KeyPurpose::FunrunDataEncryption);
    let encryptor = FunrunEncryptor::new(derived_key);

    // Too short
    let result = encryptor.decrypt(&[0u8; 5]);
    assert!(result.is_err());

    // Invalid ciphertext (wrong tag)
    let mut ciphertext = encryptor.encrypt(b"test").unwrap();
    let len = ciphertext.len();
    ciphertext[len - 1] ^= 0xff; // Corrupt the tag
    let result = encryptor.decrypt(&ciphertext);
    assert!(result.is_err());
}

#[test]
fn test_funrun_encryptor_different_keys_cannot_decrypt() {
    let secret1 = Secret::random();
    let secret2 = Secret::random();

    let derived_key1 = DerivedKey::derive(&secret1, KeyPurpose::FunrunDataEncryption);
    let derived_key2 = DerivedKey::derive(&secret2, KeyPurpose::FunrunDataEncryption);

    let encryptor1 = FunrunEncryptor::new(derived_key1);
    let encryptor2 = FunrunEncryptor::new(derived_key2);

    let plaintext = b"secret message";
    let ciphertext = encryptor1.encrypt(plaintext).unwrap();

    // Different key should fail to decrypt
    let result = encryptor2.decrypt(&ciphertext);
    assert!(result.is_err());
}
