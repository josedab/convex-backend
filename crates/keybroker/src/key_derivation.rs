// Key derivation module for purpose-specific keys using HKDF
//
// This module implements RFC-0006: Secret Key Architecture
// It provides HKDF-based key derivation for different purposes,
// following the principle of least privilege.

use aws_lc_rs::hkdf::{
    Prk,
    Salt,
    HKDF_SHA256,
};

use crate::InstanceSecret;

/// Key length for derived keys (256 bits)
pub const DERIVED_KEY_LEN: usize = 32;

/// Purposes for derived keys
///
/// Each purpose generates a different derived key from the same instance secret,
/// providing cryptographic separation between different use cases.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyPurpose {
    /// Encrypting user data in function runner
    FunrunDataEncryption,

    /// Signing auth tokens
    TokenSigning,

    /// Encrypting environment variables
    EnvVarEncryption,

    /// Encrypting file storage metadata
    StorageMetadata,

    /// Signing internal API requests
    InternalApiSigning,

    /// Encrypting cursors (deterministic)
    CursorEncryption,

    /// Encrypting query journals
    QueryJournalEncryption,

    /// Encrypting action callback tokens
    ActionCallbackEncryption,

    /// Encrypting admin keys
    AdminKeyEncryption,

    /// Encrypting store file authorizations
    StoreFileAuthorization,
}

impl KeyPurpose {
    /// Get the domain separator for this purpose
    ///
    /// Domain separators ensure that keys derived for different purposes
    /// are cryptographically independent.
    pub fn domain_separator(&self) -> &'static [u8] {
        match self {
            Self::FunrunDataEncryption => b"convex-funrun-encryption-v1",
            Self::TokenSigning => b"convex-token-signing-v1",
            Self::EnvVarEncryption => b"convex-envvar-encryption-v1",
            Self::StorageMetadata => b"convex-storage-metadata-v1",
            Self::InternalApiSigning => b"convex-internal-api-v1",
            Self::CursorEncryption => b"convex-cursor-encryption-v1",
            Self::QueryJournalEncryption => b"convex-query-journal-v1",
            Self::ActionCallbackEncryption => b"convex-action-callback-v1",
            Self::AdminKeyEncryption => b"convex-admin-key-v1",
            Self::StoreFileAuthorization => b"convex-store-file-auth-v1",
        }
    }

    /// Get a human-readable name for this purpose (for logging/debugging)
    pub fn name(&self) -> &'static str {
        match self {
            Self::FunrunDataEncryption => "funrun-data-encryption",
            Self::TokenSigning => "token-signing",
            Self::EnvVarEncryption => "envvar-encryption",
            Self::StorageMetadata => "storage-metadata",
            Self::InternalApiSigning => "internal-api-signing",
            Self::CursorEncryption => "cursor-encryption",
            Self::QueryJournalEncryption => "query-journal-encryption",
            Self::ActionCallbackEncryption => "action-callback-encryption",
            Self::AdminKeyEncryption => "admin-key-encryption",
            Self::StoreFileAuthorization => "store-file-authorization",
        }
    }
}

/// A key derived for a specific purpose
///
/// This type ensures that:
/// 1. Keys are derived using HKDF-SHA256
/// 2. Keys are zeroized when dropped (security)
/// 3. Keys are tagged with their purpose for auditing
pub struct DerivedKey {
    key: [u8; DERIVED_KEY_LEN],
    purpose: KeyPurpose,
}

impl DerivedKey {
    /// Derive a key for a specific purpose from the instance secret
    ///
    /// Uses HKDF-SHA256 with the purpose's domain separator as the salt
    /// and "derived-key" as the info parameter.
    pub fn derive(instance_secret: &InstanceSecret, purpose: KeyPurpose) -> Self {
        // Use HKDF with the domain separator as salt
        let salt = Salt::new(HKDF_SHA256, purpose.domain_separator());

        // Extract phase: create PRK from input key material
        let prk: Prk = salt.extract(instance_secret.as_bytes());

        // Expand phase: derive the output key
        let mut key = [0u8; DERIVED_KEY_LEN];
        let okm = prk
            .expand(&[b"derived-key"], HkdfKeyLen)
            .expect("HKDF expand should not fail for valid lengths");
        okm.fill(&mut key)
            .expect("HKDF fill should not fail for valid lengths");

        Self { key, purpose }
    }

    /// Get the derived key bytes
    pub fn as_bytes(&self) -> &[u8; DERIVED_KEY_LEN] {
        &self.key
    }

    /// Get the purpose this key was derived for
    pub fn purpose(&self) -> KeyPurpose {
        self.purpose
    }

    /// Create a DerivedKey from raw bytes (for deserialization)
    ///
    /// Note: This should only be used for keys that were previously derived
    /// using `DerivedKey::derive`.
    pub fn from_bytes(key: [u8; DERIVED_KEY_LEN], purpose: KeyPurpose) -> Self {
        Self { key, purpose }
    }
}

impl Clone for DerivedKey {
    fn clone(&self) -> Self {
        Self {
            key: self.key,
            purpose: self.purpose,
        }
    }
}

impl Drop for DerivedKey {
    fn drop(&mut self) {
        // Zeroize on drop for security
        // This prevents the key from lingering in memory
        self.key.iter_mut().for_each(|b| *b = 0);
    }
}

// Custom length type for HKDF output
struct HkdfKeyLen;

impl aws_lc_rs::hkdf::KeyType for HkdfKeyLen {
    fn len(&self) -> usize {
        DERIVED_KEY_LEN
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Secret;

    #[test]
    fn test_key_derivation_deterministic() {
        // Same secret + purpose should produce same key
        let secret = Secret::random();
        let key1 = DerivedKey::derive(&secret, KeyPurpose::FunrunDataEncryption);
        let key2 = DerivedKey::derive(&secret, KeyPurpose::FunrunDataEncryption);

        assert_eq!(key1.as_bytes(), key2.as_bytes());
        assert_eq!(key1.purpose(), key2.purpose());
    }

    #[test]
    fn test_different_purposes_different_keys() {
        // Same secret + different purposes should produce different keys
        let secret = Secret::random();
        let key1 = DerivedKey::derive(&secret, KeyPurpose::FunrunDataEncryption);
        let key2 = DerivedKey::derive(&secret, KeyPurpose::TokenSigning);

        assert_ne!(key1.as_bytes(), key2.as_bytes());
    }

    #[test]
    fn test_different_secrets_different_keys() {
        // Different secrets + same purpose should produce different keys
        let secret1 = Secret::random();
        let secret2 = Secret::random();
        let key1 = DerivedKey::derive(&secret1, KeyPurpose::FunrunDataEncryption);
        let key2 = DerivedKey::derive(&secret2, KeyPurpose::FunrunDataEncryption);

        assert_ne!(key1.as_bytes(), key2.as_bytes());
    }

    #[test]
    fn test_all_purposes_have_unique_separators() {
        // Ensure all purposes have unique domain separators
        let purposes = [
            KeyPurpose::FunrunDataEncryption,
            KeyPurpose::TokenSigning,
            KeyPurpose::EnvVarEncryption,
            KeyPurpose::StorageMetadata,
            KeyPurpose::InternalApiSigning,
            KeyPurpose::CursorEncryption,
            KeyPurpose::QueryJournalEncryption,
            KeyPurpose::ActionCallbackEncryption,
            KeyPurpose::AdminKeyEncryption,
            KeyPurpose::StoreFileAuthorization,
        ];

        let separators: Vec<_> = purposes.iter().map(|p| p.domain_separator()).collect();

        // Check uniqueness
        for (i, sep1) in separators.iter().enumerate() {
            for (j, sep2) in separators.iter().enumerate() {
                if i != j {
                    assert_ne!(
                        sep1, sep2,
                        "Purposes {:?} and {:?} have the same domain separator",
                        purposes[i], purposes[j]
                    );
                }
            }
        }
    }

    #[test]
    fn test_derived_key_clone() {
        let secret = Secret::random();
        let key1 = DerivedKey::derive(&secret, KeyPurpose::FunrunDataEncryption);
        let key2 = key1.clone();

        assert_eq!(key1.as_bytes(), key2.as_bytes());
        assert_eq!(key1.purpose(), key2.purpose());
    }

    #[test]
    fn test_derived_key_from_bytes() {
        let secret = Secret::random();
        let key1 = DerivedKey::derive(&secret, KeyPurpose::TokenSigning);
        let key2 = DerivedKey::from_bytes(*key1.as_bytes(), KeyPurpose::TokenSigning);

        assert_eq!(key1.as_bytes(), key2.as_bytes());
        assert_eq!(key1.purpose(), key2.purpose());
    }

    #[test]
    fn test_key_purpose_names() {
        // Ensure all purposes have names
        let purposes = [
            KeyPurpose::FunrunDataEncryption,
            KeyPurpose::TokenSigning,
            KeyPurpose::EnvVarEncryption,
            KeyPurpose::StorageMetadata,
            KeyPurpose::InternalApiSigning,
        ];

        for purpose in &purposes {
            assert!(!purpose.name().is_empty());
        }
    }
}
