# RFC-0006: Secret Key Architecture

**Status:** Draft
**Author:** Claude (AI Analysis)
**Created:** November 19, 2025
**Analysis Commit:** `482df8a0e1288c9410be8241deb06b09d7ff70b0`

---

## Summary

Improve security by deriving purpose-specific keys from the instance secret rather than passing the full secret to the function runner. This follows the principle of least privilege and reduces the blast radius of potential compromises.

## Motivation

### Current Issue

**Location:** `crates/keybroker/src/encryptor.rs:74`

```rust
pub fn create_funrun_encryptor(instance_secret: &InstanceSecret) -> FunrunEncryptor {
    // TODO: do not send instance secrets to funrun, only derived keys
    //
    // Currently we send the full instance secret to the function runner,
    // which has broader access than it needs.
    FunrunEncryptor {
        key: instance_secret.clone(),
    }
}
```

### Security Problems

1. **Excessive Privileges**: Function runner has access to all key operations
2. **Blast Radius**: Compromise of funrun exposes all secrets
3. **No Key Separation**: Same key for different purposes
4. **Audit Difficulty**: Cannot track key usage by purpose
5. **Rotation Complexity**: Must rotate everything together

### Attack Scenarios

**Scenario 1: Function Runner Compromise**
```
Current: Attacker gets instance secret → Can decrypt ALL data
Proposed: Attacker gets derived key → Can only decrypt funrun-specific data
```

**Scenario 2: Log Leakage**
```
Current: Instance secret in logs → Full compromise
Proposed: Derived key in logs → Limited compromise
```

## Detailed Design

### Key Derivation Using HKDF

Use HKDF (HMAC-based Key Derivation Function) to derive purpose-specific keys:

```rust
// crates/keybroker/src/key_derivation.rs

use hkdf::Hkdf;
use sha2::Sha256;

/// Purposes for derived keys
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
}

impl KeyPurpose {
    /// Get the domain separator for this purpose
    pub fn domain_separator(&self) -> &'static [u8] {
        match self {
            Self::FunrunDataEncryption => b"convex-funrun-encryption-v1",
            Self::TokenSigning => b"convex-token-signing-v1",
            Self::EnvVarEncryption => b"convex-envvar-encryption-v1",
            Self::StorageMetadata => b"convex-storage-metadata-v1",
            Self::InternalApiSigning => b"convex-internal-api-v1",
        }
    }
}

/// A key derived for a specific purpose
pub struct DerivedKey {
    key: [u8; 32],
    purpose: KeyPurpose,
}

impl DerivedKey {
    /// Derive a key for a specific purpose from the instance secret
    pub fn derive(
        instance_secret: &InstanceSecret,
        purpose: KeyPurpose,
    ) -> Self {
        let hk = Hkdf::<Sha256>::new(
            Some(purpose.domain_separator()),  // salt
            instance_secret.as_bytes(),        // input key material
        );

        let mut key = [0u8; 32];
        hk.expand(b"derived-key", &mut key)
            .expect("32 bytes is valid for HKDF-SHA256");

        Self { key, purpose }
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.key
    }

    pub fn purpose(&self) -> KeyPurpose {
        self.purpose
    }
}

impl Drop for DerivedKey {
    fn drop(&mut self) {
        // Zeroize on drop for security
        self.key.iter_mut().for_each(|b| *b = 0);
    }
}
```

### Updated KeyBroker

```rust
// crates/keybroker/src/lib.rs

pub struct KeyBroker {
    instance_secret: InstanceSecret,
    // Cache derived keys
    derived_keys: Mutex<HashMap<KeyPurpose, DerivedKey>>,
}

impl KeyBroker {
    pub fn new(instance_secret: InstanceSecret) -> Self {
        Self {
            instance_secret,
            derived_keys: Mutex::new(HashMap::new()),
        }
    }

    /// Get a derived key for a specific purpose
    pub fn get_derived_key(&self, purpose: KeyPurpose) -> DerivedKey {
        let mut cache = self.derived_keys.lock().unwrap();

        if let Some(key) = cache.get(&purpose) {
            return key.clone();
        }

        let key = DerivedKey::derive(&self.instance_secret, purpose);
        cache.insert(purpose, key.clone());
        key
    }

    /// Create encryptor for function runner (UPDATED)
    pub fn create_funrun_encryptor(&self) -> FunrunEncryptor {
        let key = self.get_derived_key(KeyPurpose::FunrunDataEncryption);
        FunrunEncryptor::new(key)
    }

    /// Create signer for auth tokens
    pub fn create_token_signer(&self) -> TokenSigner {
        let key = self.get_derived_key(KeyPurpose::TokenSigning);
        TokenSigner::new(key)
    }

    /// Create encryptor for environment variables
    pub fn create_envvar_encryptor(&self) -> EnvVarEncryptor {
        let key = self.get_derived_key(KeyPurpose::EnvVarEncryption);
        EnvVarEncryptor::new(key)
    }
}
```

### Updated Function Runner

```rust
// crates/function_runner/src/lib.rs

pub struct FunctionRunner {
    // Only has the derived key, not the instance secret
    encryptor: FunrunEncryptor,
}

impl FunctionRunner {
    pub fn new(encryptor: FunrunEncryptor) -> Self {
        Self { encryptor }
    }

    pub fn encrypt_result(&self, data: &[u8]) -> anyhow::Result<Vec<u8>> {
        self.encryptor.encrypt(data)
    }

    pub fn decrypt_args(&self, data: &[u8]) -> anyhow::Result<Vec<u8>> {
        self.encryptor.decrypt(data)
    }
}
```

### Updated Encryptor

```rust
// crates/keybroker/src/encryptor.rs

pub struct FunrunEncryptor {
    derived_key: DerivedKey,
}

impl FunrunEncryptor {
    pub fn new(derived_key: DerivedKey) -> Self {
        assert_eq!(
            derived_key.purpose(),
            KeyPurpose::FunrunDataEncryption,
            "Wrong key purpose for FunrunEncryptor"
        );
        Self { derived_key }
    }

    pub fn encrypt(&self, plaintext: &[u8]) -> anyhow::Result<Vec<u8>> {
        // Use AES-256-GCM with the derived key
        use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
        use aes_gcm::aead::Aead;

        let cipher = Aes256Gcm::new_from_slice(self.derived_key.as_bytes())?;

        // Generate random nonce
        let nonce_bytes: [u8; 12] = rand::random();
        let nonce = Nonce::from_slice(&nonce_bytes);

        let ciphertext = cipher.encrypt(nonce, plaintext)?;

        // Prepend nonce to ciphertext
        let mut result = nonce_bytes.to_vec();
        result.extend(ciphertext);
        Ok(result)
    }

    pub fn decrypt(&self, ciphertext: &[u8]) -> anyhow::Result<Vec<u8>> {
        use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
        use aes_gcm::aead::Aead;

        if ciphertext.len() < 12 {
            anyhow::bail!("Ciphertext too short");
        }

        let (nonce_bytes, encrypted) = ciphertext.split_at(12);
        let nonce = Nonce::from_slice(nonce_bytes);

        let cipher = Aes256Gcm::new_from_slice(self.derived_key.as_bytes())?;
        let plaintext = cipher.decrypt(nonce, encrypted)?;

        Ok(plaintext)
    }
}
```

### Key Rotation Support

```rust
// crates/keybroker/src/rotation.rs

pub struct KeyRotation {
    current_secret: InstanceSecret,
    previous_secret: Option<InstanceSecret>,
    rotation_time: SystemTime,
}

impl KeyRotation {
    /// Decrypt with current or previous key
    pub fn decrypt_with_fallback(
        &self,
        purpose: KeyPurpose,
        ciphertext: &[u8],
    ) -> anyhow::Result<Vec<u8>> {
        // Try current key first
        let current_key = DerivedKey::derive(&self.current_secret, purpose);
        let encryptor = FunrunEncryptor::new(current_key);

        if let Ok(plaintext) = encryptor.decrypt(ciphertext) {
            return Ok(plaintext);
        }

        // Fall back to previous key if available
        if let Some(previous) = &self.previous_secret {
            let previous_key = DerivedKey::derive(previous, purpose);
            let encryptor = FunrunEncryptor::new(previous_key);
            return encryptor.decrypt(ciphertext);
        }

        anyhow::bail!("Decryption failed with all available keys")
    }
}
```

## Example Usage

### Before (Insecure)

```rust
// Application startup
let instance_secret = load_instance_secret();

// Function runner gets full secret
let funrun_encryptor = FunrunEncryptor {
    key: instance_secret.clone(),  // FULL ACCESS
};

let runner = FunctionRunner::new(funrun_encryptor);
// runner can now derive any key or perform any operation
```

### After (Secure)

```rust
// Application startup
let instance_secret = load_instance_secret();
let key_broker = KeyBroker::new(instance_secret);

// Function runner gets derived key only
let funrun_encryptor = key_broker.create_funrun_encryptor();
// Only has KeyPurpose::FunrunDataEncryption

let runner = FunctionRunner::new(funrun_encryptor);
// runner can only encrypt/decrypt funrun data
```

## Implementation Plan

### Milestones

| Phase | Duration | Deliverable |
|-------|----------|-------------|
| 1. Key derivation | 3 days | HKDF implementation |
| 2. KeyBroker update | 2 days | Purpose-based keys |
| 3. Encryptor update | 2 days | Use derived keys |
| 4. Migration | 3 days | Re-encrypt existing data |
| 5. Rotation support | 2 days | Key rotation logic |
| 6. Audit & review | 3 days | Security review |

### Testing Strategy

1. **Unit Tests**
   - Key derivation determinism
   - Encrypt/decrypt round-trip
   - Purpose enforcement
   - Rotation fallback

2. **Integration Tests**
   - End-to-end function execution
   - Cross-component encryption

3. **Security Tests**
   - Cannot derive other purposes
   - Cannot extract instance secret
   - Zeroization on drop

### Migration Plan

1. **Phase 1**: Deploy new key derivation alongside old
2. **Phase 2**: Migrate encryption to derived keys
3. **Phase 3**: Re-encrypt existing data
4. **Phase 4**: Remove old code paths

## Backwards Compatibility

### Breaking Changes

None for users. Internal change only.

### Data Migration

Existing encrypted data must be re-encrypted:

```rust
pub async fn migrate_encrypted_data(
    old_key: &InstanceSecret,
    new_broker: &KeyBroker,
) -> anyhow::Result<()> {
    // For each encrypted item:
    // 1. Decrypt with old key
    // 2. Re-encrypt with new derived key
    // 3. Update storage
}
```

## Alternatives Considered

### 1. Hardware Security Module (HSM)

Use HSM for key management.

**Pros:** High security
**Cons:**
- Cost
- Complexity
- Latency
- Not available in all environments

### 2. External Key Management Service

Use AWS KMS or similar.

**Pros:** Managed service
**Cons:**
- Network dependency
- Cost
- Vendor lock-in

### 3. Per-Instance Keys

Generate separate keys for each purpose at deployment.

**Pros:** No derivation needed
**Cons:**
- More keys to manage
- Rotation complexity
- Key distribution

HKDF-based derivation is the best balance of security and simplicity.

## Open Questions

1. **How to handle key rotation?**
   - Recommendation: Support previous key for decryption window

2. **Should we use different KDF for different purposes?**
   - Recommendation: No, HKDF-SHA256 is sufficient for all

3. **What about performance impact?**
   - Key derivation is fast (~microseconds)
   - Cache derived keys to avoid repeated derivation

## Success Criteria

- [ ] Instance secret never sent to function runner
- [ ] All encryption uses purpose-derived keys
- [ ] Key rotation works without data loss
- [ ] Security review passed
- [ ] No performance regression > 1%
- [ ] Existing data migrated successfully

## Effort Estimation

| Task | Effort |
|------|--------|
| Key derivation | 2 dev-days |
| KeyBroker update | 1 dev-day |
| Encryptor update | 1 dev-day |
| Migration | 2 dev-days |
| Rotation support | 1 dev-day |
| Testing | 2 dev-days |
| Security review | 3 dev-days |

**Total: ~3 weeks with 1 engineer** (includes security review)

## Stakeholders

- **Engineering**: Required - implementation
- **Security**: Required - review and approval
- **DevOps**: Interested - key management
- **Compliance**: Interested - audit trail

---

*References:*
- [Keybroker](https://github.com/get-convex/convex-backend/blob/482df8a0e1288c9410be8241deb06b09d7ff70b0/crates/keybroker/src/encryptor.rs)
- [HKDF RFC 5869](https://tools.ietf.org/html/rfc5869)
