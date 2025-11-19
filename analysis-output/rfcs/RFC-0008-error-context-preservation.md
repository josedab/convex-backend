# RFC-0008: Error Context Preservation

**Status:** Draft
**Author:** Claude (AI Analysis)
**Created:** November 19, 2025
**Analysis Commit:** `482df8a0e1288c9410be8241deb06b09d7ff70b0`

---

## Summary

Ensure error context is preserved throughout the error handling chain to improve debugging and developer experience.

## Motivation

### Lost Context

**Location:** `crates/common/src/schemas/validator.rs:275`
```rust
// TODO: This is dropping the error messages from the individual
```

### Impact

- **Poor debugging**: Root cause is hidden
- **Bad DX**: Developers see generic errors
- **Support burden**: More investigation needed

## Detailed Design

### Current Problem

```rust
fn validate_schema(fields: &[Field]) -> Result<(), ValidationError> {
    let errors: Vec<_> = fields
        .iter()
        .filter_map(|f| validate_field(f).err())
        .collect();

    if !errors.is_empty() {
        // BAD: Loses individual error messages
        return Err(ValidationError::InvalidSchema);
    }
    Ok(())
}
```

### Solution

```rust
fn validate_schema(fields: &[Field]) -> Result<(), ValidationError> {
    let errors: Vec<_> = fields
        .iter()
        .filter_map(|f| validate_field(f).err())
        .collect();

    if !errors.is_empty() {
        // GOOD: Preserves all context
        return Err(ValidationError::MultipleErrors {
            errors,
            summary: format!("{} field(s) failed validation", errors.len()),
        });
    }
    Ok(())
}
```

## Implementation Plan

1. Audit error handling paths (1 day)
2. Fix context preservation (1 day)

## Success Criteria

- [ ] All validation errors include field-level details
- [ ] Error chains preserved through all paths
- [ ] No generic "something went wrong" errors

## Effort Estimation

**Total: 2 dev-days**

---

*References:*
- [Validator](https://github.com/get-convex/convex-backend/blob/482df8a0e1288c9410be8241deb06b09d7ff70b0/crates/common/src/schemas/validator.rs)
