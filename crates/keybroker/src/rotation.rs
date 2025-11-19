// Key rotation support for secret key architecture
//
// This module implements key rotation functionality as specified in RFC-0006.
// It allows decryption with both current and previous keys during rotation periods.

use std::time::SystemTime;

use anyhow::Context;

use crate::{
    key_derivation::{
        DerivedKey,
        KeyPurpose,
    },
    InstanceSecret,
};

/// Manages key rotation by maintaining current and previous instance secrets
///
/// During a key rotation period, this struct allows:
/// 1. Encryption with the current key only
/// 2. Decryption with current or previous key (fallback)
///
/// This ensures zero downtime during key rotations.
pub struct KeyRotation {
    current_secret: InstanceSecret,
    previous_secret: Option<InstanceSecret>,
    rotation_time: SystemTime,
}

impl KeyRotation {
    /// Create a new KeyRotation with only a current secret
    pub fn new(current_secret: InstanceSecret) -> Self {
        Self {
            current_secret,
            previous_secret: None,
            rotation_time: SystemTime::now(),
        }
    }

    /// Create a new KeyRotation with both current and previous secrets
    ///
    /// Use this when rotating to a new key while maintaining decryption
    /// capability for data encrypted with the old key.
    pub fn with_previous(
        current_secret: InstanceSecret,
        previous_secret: InstanceSecret,
        rotation_time: SystemTime,
    ) -> Self {
        Self {
            current_secret,
            previous_secret: Some(previous_secret),
            rotation_time,
        }
    }

    /// Get the current secret
    pub fn current_secret(&self) -> &InstanceSecret {
        &self.current_secret
    }

    /// Get the previous secret if available
    pub fn previous_secret(&self) -> Option<&InstanceSecret> {
        self.previous_secret.as_ref()
    }

    /// Get the time of the last rotation
    pub fn rotation_time(&self) -> SystemTime {
        self.rotation_time
    }

    /// Check if we're in a rotation period (have a previous secret)
    pub fn is_rotating(&self) -> bool {
        self.previous_secret.is_some()
    }

    /// Derive a key from the current secret for a specific purpose
    pub fn derive_current_key(&self, purpose: KeyPurpose) -> DerivedKey {
        DerivedKey::derive(&self.current_secret, purpose)
    }

    /// Derive a key from the previous secret for a specific purpose
    ///
    /// Returns None if there is no previous secret.
    pub fn derive_previous_key(&self, purpose: KeyPurpose) -> Option<DerivedKey> {
        self.previous_secret
            .as_ref()
            .map(|secret| DerivedKey::derive(secret, purpose))
    }

    /// Try to decrypt with current key, falling back to previous if available
    ///
    /// This is the main method for decryption during key rotation.
    /// It attempts decryption with the current key first, then falls back
    /// to the previous key if decryption fails and a previous key exists.
    pub fn decrypt_with_fallback<F, T>(
        &self,
        purpose: KeyPurpose,
        mut decrypt_fn: F,
    ) -> anyhow::Result<T>
    where
        F: FnMut(&DerivedKey) -> anyhow::Result<T>,
    {
        // Try current key first
        let current_key = self.derive_current_key(purpose);
        if let Ok(result) = decrypt_fn(&current_key) {
            return Ok(result);
        }

        // Fall back to previous key if available
        if let Some(previous_key) = self.derive_previous_key(purpose) {
            return decrypt_fn(&previous_key).context(
                "Decryption failed with both current and previous keys during rotation",
            );
        }

        // No previous key, fail with current key error
        decrypt_fn(&current_key)
    }

    /// Complete the rotation by discarding the previous secret
    ///
    /// Call this after all data has been re-encrypted with the new key.
    pub fn complete_rotation(self) -> Self {
        Self {
            current_secret: self.current_secret,
            previous_secret: None,
            rotation_time: self.rotation_time,
        }
    }

    /// Start a new rotation with a new secret
    ///
    /// The current secret becomes the previous secret.
    pub fn rotate_to(self, new_secret: InstanceSecret) -> Self {
        Self {
            current_secret: new_secret,
            previous_secret: Some(self.current_secret),
            rotation_time: SystemTime::now(),
        }
    }
}

impl Clone for KeyRotation {
    fn clone(&self) -> Self {
        Self {
            current_secret: self.current_secret,
            previous_secret: self.previous_secret,
            rotation_time: self.rotation_time,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Secret;

    #[test]
    fn test_key_rotation_new() {
        let secret = Secret::random();
        let rotation = KeyRotation::new(secret);

        assert!(!rotation.is_rotating());
        assert!(rotation.previous_secret().is_none());
    }

    #[test]
    fn test_key_rotation_with_previous() {
        let current = Secret::random();
        let previous = Secret::random();
        let rotation_time = SystemTime::now();

        let rotation = KeyRotation::with_previous(current, previous, rotation_time);

        assert!(rotation.is_rotating());
        assert!(rotation.previous_secret().is_some());
        assert_eq!(rotation.rotation_time(), rotation_time);
    }

    #[test]
    fn test_derive_current_key() {
        let secret = Secret::random();
        let rotation = KeyRotation::new(secret);

        let key = rotation.derive_current_key(KeyPurpose::FunrunDataEncryption);
        assert_eq!(key.purpose(), KeyPurpose::FunrunDataEncryption);
    }

    #[test]
    fn test_derive_previous_key() {
        let current = Secret::random();
        let previous = Secret::random();
        let rotation = KeyRotation::with_previous(current, previous, SystemTime::now());

        let prev_key = rotation.derive_previous_key(KeyPurpose::TokenSigning);
        assert!(prev_key.is_some());
        assert_eq!(prev_key.unwrap().purpose(), KeyPurpose::TokenSigning);
    }

    #[test]
    fn test_decrypt_with_fallback_current_success() {
        let secret = Secret::random();
        let rotation = KeyRotation::new(secret);

        let result = rotation.decrypt_with_fallback(KeyPurpose::FunrunDataEncryption, |key| {
            // Simulate successful decryption with current key
            Ok(key.purpose())
        });

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), KeyPurpose::FunrunDataEncryption);
    }

    #[test]
    fn test_decrypt_with_fallback_to_previous() {
        let current = Secret::random();
        let previous = Secret::random();
        let rotation = KeyRotation::with_previous(current, previous, SystemTime::now());

        let mut attempts = 0;
        let result = rotation.decrypt_with_fallback(KeyPurpose::FunrunDataEncryption, |_key| {
            attempts += 1;
            if attempts == 1 {
                // First attempt (current key) fails
                anyhow::bail!("Current key failed")
            } else {
                // Second attempt (previous key) succeeds
                Ok("success")
            }
        });

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "success");
        assert_eq!(attempts, 2);
    }

    #[test]
    fn test_complete_rotation() {
        let current = Secret::random();
        let previous = Secret::random();
        let rotation = KeyRotation::with_previous(current, previous, SystemTime::now());

        assert!(rotation.is_rotating());

        let completed = rotation.complete_rotation();
        assert!(!completed.is_rotating());
        assert!(completed.previous_secret().is_none());
    }

    #[test]
    fn test_rotate_to() {
        let initial = Secret::random();
        let new_secret = Secret::random();

        let rotation = KeyRotation::new(initial);
        assert!(!rotation.is_rotating());

        let rotated = rotation.rotate_to(new_secret);
        assert!(rotated.is_rotating());
        assert!(rotated.previous_secret().is_some());
    }

    #[test]
    fn test_clone() {
        let current = Secret::random();
        let previous = Secret::random();
        let rotation = KeyRotation::with_previous(current, previous, SystemTime::now());

        let cloned = rotation.clone();
        assert_eq!(rotation.is_rotating(), cloned.is_rotating());
        assert_eq!(
            rotation.current_secret().as_bytes(),
            cloned.current_secret().as_bytes()
        );
    }
}
