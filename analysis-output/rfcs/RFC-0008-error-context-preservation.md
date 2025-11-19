# RFC-0008: Error Context Preservation

**Status:** Draft
**Author:** Claude (AI Analysis)
**Created:** November 19, 2025
**Analysis Commit:** `482df8a0e1288c9410be8241deb06b09d7ff70b0`

---

## Summary

Ensure error context is preserved throughout all error handling paths to improve debugging experience and provide actionable error messages to developers.

## Motivation

### Lost Context Issue

**Location:** `crates/common/src/schemas/validator.rs:275`

```rust
fn validate_document(doc: &Document, schema: &Schema) -> Result<(), ValidationError> {
    let field_errors: Vec<_> = schema.fields()
        .iter()
        .filter_map(|field| {
            validate_field(doc, field).err()
        })
        .collect();

    if !field_errors.is_empty() {
        // TODO: This is dropping the error messages from the individual
        // field validations. We should preserve them.
        return Err(ValidationError::InvalidDocument);
    }

    Ok(())
}
```

### Impact

1. **Poor Debugging**: Developers see "validation failed" without knowing which field
2. **Increased Support**: More questions about vague errors
3. **Wasted Time**: Developers add logging to figure out root cause
4. **Bad DX**: Frustrating developer experience

### Other Affected Areas

Similar patterns exist in:
- Schema validation
- Type checking
- Configuration parsing
- API request validation

## Detailed Design

### Create Rich Error Types

**Updated ValidationError:**

```rust
// crates/common/src/errors/validation.rs

/// Error from validating a document against a schema
#[derive(Debug, thiserror::Error)]
pub enum ValidationError {
    #[error("Document validation failed:\n{}", format_field_errors(.0))]
    InvalidDocument(Vec<FieldError>),

    #[error("Schema validation failed:\n{}", format_schema_errors(.0))]
    InvalidSchema(Vec<SchemaError>),

    #[error("Type mismatch: expected {expected}, got {actual}")]
    TypeMismatch {
        expected: String,
        actual: String,
        path: String,
    },
}

/// Error for a specific field
#[derive(Debug)]
pub struct FieldError {
    pub field_path: String,
    pub error: FieldValidationError,
}

#[derive(Debug, thiserror::Error)]
pub enum FieldValidationError {
    #[error("required field is missing")]
    MissingRequired,

    #[error("expected type {expected}, got {actual}")]
    WrongType { expected: String, actual: String },

    #[error("value {value} is not in allowed set: {allowed:?}")]
    NotInAllowedSet { value: String, allowed: Vec<String> },

    #[error("string length {actual} exceeds maximum {max}")]
    StringTooLong { actual: usize, max: usize },

    #[error("value {value} is less than minimum {min}")]
    BelowMinimum { value: f64, min: f64 },

    #[error("value {value} exceeds maximum {max}")]
    AboveMaximum { value: f64, max: f64 },

    #[error("nested validation failed: {0}")]
    Nested(Box<ValidationError>),
}

fn format_field_errors(errors: &[FieldError]) -> String {
    errors
        .iter()
        .map(|e| format!("  - {}: {}", e.field_path, e.error))
        .collect::<Vec<_>>()
        .join("\n")
}
```

### Update Validation Logic

```rust
// crates/common/src/schemas/validator.rs

fn validate_document(
    doc: &Document,
    schema: &Schema
) -> Result<(), ValidationError> {
    let mut field_errors = Vec::new();

    for field in schema.fields() {
        if let Err(error) = validate_field(doc, field) {
            field_errors.push(FieldError {
                field_path: field.path.clone(),
                error,
            });
        }
    }

    if !field_errors.is_empty() {
        return Err(ValidationError::InvalidDocument(field_errors));
    }

    Ok(())
}

fn validate_field(
    doc: &Document,
    field: &FieldSchema
) -> Result<(), FieldValidationError> {
    let value = doc.get(&field.path);

    // Check required
    if field.required && value.is_none() {
        return Err(FieldValidationError::MissingRequired);
    }

    let Some(value) = value else {
        return Ok(());
    };

    // Check type
    if !field.field_type.matches(value) {
        return Err(FieldValidationError::WrongType {
            expected: field.field_type.name().to_string(),
            actual: value.type_name().to_string(),
        });
    }

    // Check constraints
    match &field.constraints {
        Some(Constraints::String { max_length }) => {
            if let ConvexValue::String(s) = value {
                if s.len() > *max_length {
                    return Err(FieldValidationError::StringTooLong {
                        actual: s.len(),
                        max: *max_length,
                    });
                }
            }
        }
        Some(Constraints::Number { min, max }) => {
            if let ConvexValue::Float(n) = value {
                if let Some(min) = min {
                    if n < min {
                        return Err(FieldValidationError::BelowMinimum {
                            value: *n,
                            min: *min,
                        });
                    }
                }
                if let Some(max) = max {
                    if n > max {
                        return Err(FieldValidationError::AboveMaximum {
                            value: *n,
                            max: *max,
                        });
                    }
                }
            }
        }
        // ... more constraint types
        None => {}
    }

    Ok(())
}
```

### Preserve Context in Error Chains

Use `anyhow` context properly:

```rust
// Good: Preserves context
fn process_request(request: &Request) -> anyhow::Result<Response> {
    let config = parse_config(&request.config)
        .context("Failed to parse request config")?;

    let validated = validate_input(&request.input)
        .with_context(|| format!(
            "Input validation failed for request {}",
            request.id
        ))?;

    let result = execute(validated)
        .with_context(|| format!(
            "Execution failed for request {}",
            request.id
        ))?;

    Ok(result)
}

// Bad: Loses context
fn process_request_bad(request: &Request) -> anyhow::Result<Response> {
    let config = parse_config(&request.config)?;  // No context
    let validated = validate_input(&request.input)?;  // No context
    let result = execute(validated)?;  // No context
    Ok(result)
}
```

### Create Error Display Helpers

```rust
// crates/common/src/errors/display.rs

/// Format an error with its full context chain
pub fn format_error_chain(error: &anyhow::Error) -> String {
    let mut result = String::new();

    // Root error
    result.push_str(&format!("Error: {}\n", error));

    // Context chain
    for (i, cause) in error.chain().skip(1).enumerate() {
        result.push_str(&format!("  {}: {}\n", i + 1, cause));
    }

    result
}

/// Format an error for user display (less technical)
pub fn format_user_error(error: &anyhow::Error) -> String {
    // Just the top-level message
    error.to_string()
}

/// Format an error for logging (full detail)
pub fn format_log_error(error: &anyhow::Error) -> String {
    format_error_chain(error)
}
```

## Example Usage

### Before (Context Lost)

```rust
// Validation code
if !valid {
    return Err(ValidationError::InvalidDocument);
}

// Error message to user
"Error: Invalid document"

// Developer has to guess what's wrong
```

### After (Context Preserved)

```rust
// Validation code
if !valid {
    return Err(ValidationError::InvalidDocument(vec![
        FieldError {
            field_path: "user.email".to_string(),
            error: FieldValidationError::WrongType {
                expected: "string".to_string(),
                actual: "number".to_string(),
            },
        },
        FieldError {
            field_path: "user.age".to_string(),
            error: FieldValidationError::BelowMinimum {
                value: -5.0,
                min: 0.0,
            },
        },
    ]));
}

// Error message to user
"Error: Document validation failed:
  - user.email: expected type string, got number
  - user.age: value -5 is less than minimum 0"

// Developer knows exactly what to fix
```

## Implementation Plan

### Milestones

| Phase | Duration | Deliverable |
|-------|----------|-------------|
| 1. Define rich error types | 0.5 days | New error enums |
| 2. Update validators | 1 day | Preserve context in validation |
| 3. Update API handlers | 0.5 days | Preserve context in handlers |
| 4. Add display helpers | 0.5 days | Formatting functions |
| 5. Testing | 0.5 days | Error message tests |

### Testing Strategy

1. **Unit Tests**: Each error type formats correctly
2. **Integration Tests**: Full error chains preserved
3. **Snapshot Tests**: Error messages match expected format

### Files to Modify

- `crates/common/src/schemas/validator.rs`
- `crates/common/src/errors/mod.rs`
- `crates/application/src/api.rs` - API error handling
- `crates/local_backend/src/public_api/` - HTTP error responses

## Backwards Compatibility

- **Error types change**: May need migration for tests comparing error messages
- **Better errors**: Users get more information (improvement)
- **No behavior change**: Same operations succeed/fail

## Alternatives Considered

### 1. Structured Logging Instead

Log detailed errors, return generic messages.

**Pros:** Simple API
**Cons:**
- Developers must check logs
- Slow iteration
- Bad DX

### 2. Error Codes Only

Return error codes, document meanings.

**Pros:** Stable API
**Cons:**
- Requires documentation lookup
- Less actionable
- Poor DX

### 3. Full Stack Traces

Include Rust stack traces in errors.

**Pros:** Maximum detail
**Cons:**
- Security risk (exposes internals)
- Overwhelming for users
- Large response size

Rich error types balance detail and usability.

## Open Questions

1. **Should we internationalize error messages?**
   - Recommendation: Not initially, add later if needed

2. **How detailed should public errors be?**
   - Recommendation: Field-level detail, no internal implementation details

3. **Should we add error codes?**
   - Recommendation: Yes, for programmatic handling (e.g., "E001")

## Success Criteria

- [ ] All validation errors include field-level details
- [ ] Error chains preserved through all paths
- [ ] No generic "something went wrong" errors
- [ ] Error messages are actionable
- [ ] Tests for error formatting

## Effort Estimation

| Task | Effort |
|------|--------|
| Rich error types | 0.5 dev-days |
| Update validators | 0.5 dev-days |
| Update handlers | 0.5 dev-days |
| Display helpers | 0.25 dev-days |
| Testing | 0.25 dev-days |

**Total: 2 dev-days**

## Stakeholders

- **Engineering**: Required - implementation
- **Product**: Interested - user experience
- **Support**: Benefit - fewer vague error tickets

---

*References:*
- [Validator](https://github.com/get-convex/convex-backend/blob/482df8a0e1288c9410be8241deb06b09d7ff70b0/crates/common/src/schemas/validator.rs)
- [Errors Module](https://github.com/get-convex/convex-backend/blob/482df8a0e1288c9410be8241deb06b09d7ff70b0/crates/errors/src/lib.rs)
