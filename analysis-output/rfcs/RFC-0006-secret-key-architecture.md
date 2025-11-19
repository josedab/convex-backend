# RFC-0006: Secret Key Architecture

**Status:** Draft
**Author:** Claude (AI Analysis)
**Created:** November 19, 2025
**Analysis Commit:** `482df8a0e1288c9410be8241deb06b09d7ff70b0`

---

## Summary

Improve security by sending derived keys instead of instance secrets to the function runner, following the principle of least privilege.

## Motivation

**Location:** `crates/keybroker/src/encryptor.rs:74`
```rust
// TODO: do not send instance secrets to funrun, only derived keys
```

### Current Problem

The function runner receives the full instance secret, which:
- Has broader access than needed
- Increases blast radius of compromise
- Violates least privilege principle

### Solution

Derive purpose-specific keys:

```rust
impl KeyBroker {
    pub fn derive_funrun_key(&self, purpose: KeyPurpose) -> DerivedKey {
        // HKDF derivation
        let salt = purpose.as_bytes();
        hkdf_derive(&self.instance_secret, salt)
    }
}

pub enum KeyPurpose {
    EncryptUserData,
    SignTokens,
    // ... specific purposes only
}
```

## Implementation Plan

1. Define key purposes
2. Implement HKDF derivation
3. Update function runner to use derived keys
4. Rotate instance secrets (migration)

## Security Benefits

- **Reduced exposure**: Compromise gives limited access
- **Key separation**: Different keys for different purposes
- **Audit trail**: Key usage is purpose-tagged

## Effort Estimation

**Total: 3 weeks** (includes careful security review)

---

*References:*
- [Keybroker](https://github.com/get-convex/convex-backend/blob/482df8a0e1288c9410be8241deb06b09d7ff70b0/crates/keybroker/src/encryptor.rs)
