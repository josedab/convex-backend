# RFC-0007: Circular Dependency Resolution

**Status:** Draft
**Author:** Claude (AI Analysis)
**Created:** November 19, 2025
**Analysis Commit:** `482df8a0e1288c9410be8241deb06b09d7ff70b0`

---

## Summary

Refactor the codebase to eliminate circular dependency workarounds that cause code duplication, increase maintenance burden, and create potential for bugs when duplicated code drifts apart.

## Motivation

### Identified Circular Dependencies

**Issue 1: common ↔ usage_tracking**

`crates/common/src/log_streaming.rs:58`:
```rust
/// Log streaming entry for usage metrics
pub struct LogStreamingUsageEntry {
    pub table_name: String,
    pub bytes_read: u64,
    pub bytes_written: u64,
    // ... more fields
}

// TODO(sarah) this is nearly identical to the type in the `usage_tracking`
// crate, but there's a dependency cycle preventing us from using it directly.
// If usage_tracking changes, this must be updated manually.
```

The cycle:
```
common → (needs usage types)
usage_tracking → common (depends on common utilities)
```

**Issue 2: search ↔ vector**

Both crates have similar structures for index management:
```rust
// In search/
pub struct SearchIndexConfig { ... }

// In vector/ (nearly identical)
pub struct VectorIndexConfig { ... }
```

**Issue 3: Fragmented segment handling**

Multiple crates have similar segment handling that should be shared.

### Impact

1. **Code Duplication**: Same types defined 2-3 times
2. **Sync Burden**: Changes must be made in multiple places
3. **Bug Risk**: Duplicates drift apart over time
4. **Compile Time**: Redundant code compilation
5. **Cognitive Load**: Developers must track multiple versions

## Detailed Design

### Strategy 1: Extract Shared Types Crate

Create a new `convex_types` crate with no dependencies:

```rust
// crates/convex_types/src/lib.rs

//! Shared types with minimal dependencies.
//!
//! This crate contains types used across multiple crates
//! to avoid circular dependencies.

pub mod usage;
pub mod streaming;
pub mod index;
```

**Usage Types:**

```rust
// crates/convex_types/src/usage.rs

/// Usage metrics for a single operation
#[derive(Clone, Debug, PartialEq)]
pub struct UsageMetrics {
    pub table_name: String,
    pub bytes_read: u64,
    pub bytes_written: u64,
    pub documents_read: u64,
    pub documents_written: u64,
}

/// Aggregated usage for billing
#[derive(Clone, Debug, Default)]
pub struct AggregatedUsage {
    pub total_bytes_read: u64,
    pub total_bytes_written: u64,
    pub total_documents: u64,
    pub by_table: HashMap<String, UsageMetrics>,
}
```

**Update Dependents:**

```rust
// crates/common/src/log_streaming.rs
use convex_types::usage::UsageMetrics;

pub struct LogStreamingEntry {
    pub usage: UsageMetrics,  // Now uses shared type
    pub timestamp: SystemTime,
}

// crates/usage_tracking/src/lib.rs
use convex_types::usage::{UsageMetrics, AggregatedUsage};

pub struct UsageTracker {
    pub aggregated: AggregatedUsage,
}
```

### Strategy 2: Trait Inversion

For more complex relationships, use trait inversion:

**Define trait in lower crate:**

```rust
// crates/common/src/traits.rs

/// Trait for reporting usage metrics
pub trait UsageReporter: Send + Sync {
    fn report(&self, metrics: &UsageMetrics);
    fn flush(&self) -> AggregatedUsage;
}

/// Trait for index configuration
pub trait IndexConfig: Send + Sync {
    fn index_type(&self) -> IndexType;
    fn fields(&self) -> &[String];
}
```

**Implement in higher crate:**

```rust
// crates/usage_tracking/src/lib.rs

impl UsageReporter for UsageTracker {
    fn report(&self, metrics: &UsageMetrics) {
        self.record(metrics);
    }

    fn flush(&self) -> AggregatedUsage {
        self.aggregate_and_reset()
    }
}

// crates/search/src/lib.rs

impl IndexConfig for SearchIndexConfig {
    fn index_type(&self) -> IndexType {
        IndexType::FullText
    }

    fn fields(&self) -> &[String] {
        &self.searchable_fields
    }
}
```

### Strategy 3: Event-Based Decoupling

For loose coupling, use events:

```rust
// crates/common/src/events.rs

/// Event emitted when usage occurs
#[derive(Clone, Debug)]
pub struct UsageEvent {
    pub metrics: UsageMetrics,
    pub source: String,
}

/// Event bus for decoupled communication
pub struct EventBus {
    subscribers: Vec<Box<dyn Fn(&UsageEvent) + Send + Sync>>,
}

impl EventBus {
    pub fn emit(&self, event: UsageEvent) {
        for subscriber in &self.subscribers {
            subscriber(&event);
        }
    }

    pub fn subscribe<F>(&mut self, handler: F)
    where
        F: Fn(&UsageEvent) + Send + Sync + 'static
    {
        self.subscribers.push(Box::new(handler));
    }
}
```

### Implementation for Each Issue

**Issue 1: common ↔ usage_tracking**

1. Create `convex_types` crate
2. Move `UsageMetrics` and related types there
3. Update `common` to use `convex_types`
4. Update `usage_tracking` to use `convex_types`
5. Remove duplicated types

**Dependency graph before:**
```
common ←─┐
    │    │ (circular via duplicated types)
    ↓    │
usage_tracking
```

**Dependency graph after:**
```
convex_types
    ↑        ↑
    │        │
common    usage_tracking
```

**Issue 2: search ↔ vector**

1. Create shared index types in `convex_types`
2. Use trait for index-specific behavior
3. Both crates implement common trait

**Issue 3: Fragmented segments**

1. Create `storage_common` module
2. Share segment handling logic
3. Both search and vector use shared module

## Example Usage

### Before (Duplicated Types)

```rust
// In common/src/log_streaming.rs
pub struct UsageEntry {
    pub table: String,
    pub bytes: u64,
}

// In usage_tracking/src/lib.rs (DUPLICATE!)
pub struct UsageEntry {
    pub table: String,
    pub bytes: u64,
}

// Must keep both in sync manually
```

### After (Shared Type)

```rust
// In convex_types/src/usage.rs
pub struct UsageEntry {
    pub table: String,
    pub bytes: u64,
}

// In common/src/log_streaming.rs
use convex_types::usage::UsageEntry;

// In usage_tracking/src/lib.rs
use convex_types::usage::UsageEntry;

// Single source of truth
```

## Implementation Plan

### Milestones

| Phase | Duration | Deliverable |
|-------|----------|-------------|
| 1. Create convex_types | 2 days | New crate with shared types |
| 2. Migrate usage types | 3 days | Remove common/usage_tracking duplication |
| 3. Migrate index types | 3 days | Unify search/vector types |
| 4. Clean up | 2 days | Remove dead code, update docs |

### Testing Strategy

1. **Compilation**: All crates compile
2. **Unit Tests**: All existing tests pass
3. **Integration**: Cross-crate operations work
4. **No Duplicates**: Search finds no duplicated types

### Files to Create

- `crates/convex_types/Cargo.toml`
- `crates/convex_types/src/lib.rs`
- `crates/convex_types/src/usage.rs`
- `crates/convex_types/src/index.rs`
- `crates/convex_types/src/streaming.rs`

### Files to Modify

- `crates/common/src/log_streaming.rs` - Use shared types
- `crates/usage_tracking/src/lib.rs` - Use shared types
- `crates/search/src/lib.rs` - Use shared index types
- `crates/vector/src/lib.rs` - Use shared index types

## Backwards Compatibility

- **No API changes**: Types are internal
- **Same functionality**: Behavior unchanged
- **Better maintainability**: Single source of truth

## Alternatives Considered

### 1. Accept Duplication

Keep the current duplicated types.

**Pros:** No refactoring effort
**Cons:**
- Maintenance burden continues
- Bug risk from drift
- Code smell

### 2. Merge Crates

Combine common and usage_tracking.

**Pros:** No circular dependency
**Cons:**
- Creates monolithic crate
- Violates separation of concerns
- Larger compile times

### 3. Conditional Compilation

Use feature flags to selectively include code.

**Pros:** Single definition
**Cons:**
- Complex build configuration
- Confusing for developers

The shared types crate is the cleanest solution.

## Open Questions

1. **What should the new crate be named?**
   - Options: convex_types, convex_core, shared_types
   - Recommendation: `convex_types`

2. **Should we also extract traits?**
   - Recommendation: Yes, for commonly implemented traits

3. **How to handle versioning?**
   - Recommendation: Same version as workspace

## Success Criteria

- [ ] 0 TODO comments about circular dependencies
- [ ] No duplicated type definitions
- [ ] Clean dependency DAG (no cycles)
- [ ] All tests pass
- [ ] Compile time not increased

## Effort Estimation

| Task | Effort |
|------|--------|
| Create convex_types crate | 1 dev-day |
| Migrate usage types | 1.5 dev-days |
| Migrate index types | 1.5 dev-days |
| Update dependents | 1 dev-day |
| Testing | 1 dev-day |
| Documentation | 0.5 dev-days |

**Total: ~2 weeks with 1 engineer**

## Stakeholders

- **Engineering**: Required - implementation
- **Build/Infra**: Interested - compile time impact

---

*References:*
- [Log Streaming](https://github.com/get-convex/convex-backend/blob/482df8a0e1288c9410be8241deb06b09d7ff70b0/crates/common/src/log_streaming.rs)
- [Usage Tracking](https://github.com/get-convex/convex-backend/blob/482df8a0e1288c9410be8241deb06b09d7ff70b0/crates/usage_tracking/src/lib.rs)
