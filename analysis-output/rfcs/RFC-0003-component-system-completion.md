# RFC-0003: Component System Completion

**Status:** Draft
**Author:** Claude (AI Analysis)
**Created:** November 19, 2025
**Analysis Commit:** `482df8a0e1288c9410be8241deb06b09d7ff70b0`

---

## Summary

Complete the component system implementation by removing all `by_component_TODO` placeholders and adding missing functionality like HTTP action support in components.

## Motivation

The codebase contains multiple incomplete component system references:

### `by_component_TODO` Placeholders

**Location:** `crates/value/src/table_mapping.rs:53`
```rust
pub const fn by_component_TODO() -> Self
```

Found in 4+ locations:
- `crates/application/src/lib.rs:2586,3285`
- `crates/application/src/application_function_runner/mod.rs:2138`

### HTTP Actions in Components

**Location:** `crates/application/src/function_log.rs:207,1091`
```rust
// TODO(ENG-7612): Support HTTP actions in components.
```

### Component Child Seeding

**Location:** `crates/application/src/tests/components.rs:293`
```rust
// TODO: the child's random seed should depend on the parent's, so the
```

### Impact

- **Feature incomplete**: Components can't use full functionality
- **Code smell**: TODO placeholders in production code
- **Developer confusion**: Unclear what works with components

## Detailed Design

### Phase 1: Audit and Document

1. Find all component-related TODOs
2. Categorize by type (missing feature, placeholder, optimization)
3. Document intended behavior for each
4. Create test cases for expected behavior

### Phase 2: Table Mapping

Replace `by_component_TODO()` with proper component resolution:

```rust
// Before
impl TableMapping {
    pub const fn by_component_TODO() -> Self {
        // Placeholder for component namespace
    }
}

// After
impl TableMapping {
    pub fn for_component(component_id: ComponentId) -> Self {
        Self {
            namespace: Namespace::Component(component_id),
            // ...
        }
    }
}

// Usage
let mapping = TableMapping::for_component(ctx.component_id());
```

### Phase 3: HTTP Actions in Components

Enable HTTP actions to run within component contexts:

```rust
// File: crates/application/src/http_actions.rs

pub async fn execute_http_action(
    &self,
    component_id: Option<ComponentId>,  // NEW
    path: FunctionPath,
    request: HttpActionRequest,
) -> anyhow::Result<HttpActionResponse> {
    let context = if let Some(cid) = component_id {
        ExecutionContext::for_component(cid)
    } else {
        ExecutionContext::root()
    };

    // Execute with component context
    self.runner.run_http_action(context, path, request).await
}
```

### Phase 4: Component Determinism

Ensure child components have deterministic seeding:

```rust
impl ComponentContext {
    pub fn child_seed(&self, child_name: &str) -> u64 {
        // Derive seed from parent + child name
        let mut hasher = DefaultHasher::new();
        hasher.write_u64(self.seed);
        hasher.write(child_name.as_bytes());
        hasher.finish()
    }
}
```

## Implementation Plan

### Milestones

| Phase | Duration | Deliverable |
|-------|----------|-------------|
| 1. Audit | 1 day | Complete list of component TODOs |
| 2. Table mapping | 3 days | Remove by_component_TODO |
| 3. HTTP actions | 4 days | HTTP actions in components |
| 4. Determinism | 2 days | Deterministic child seeding |

### Testing Strategy

1. **Unit tests**: Each new component feature
2. **Integration tests**: Component hierarchy operations
3. **Existing tests**: Ensure no regressions

### Files to Modify

- `crates/value/src/table_mapping.rs`
- `crates/application/src/lib.rs`
- `crates/application/src/application_function_runner/mod.rs`
- `crates/application/src/function_log.rs`
- `crates/local_backend/src/http_actions.rs`

## Backwards Compatibility

- **Additive**: New component features
- **No breaking changes**: Existing code continues to work
- **Migration**: None required

## Success Criteria

- [ ] 0 `by_component_TODO` in codebase
- [ ] HTTP actions work in components
- [ ] Child component seeding is deterministic
- [ ] All component tests pass

## Effort Estimation

**Total: ~2 weeks with 1 engineer**

---

*References:*
- [Table Mapping](https://github.com/get-convex/convex-backend/blob/482df8a0e1288c9410be8241deb06b09d7ff70b0/crates/value/src/table_mapping.rs)
- [Application](https://github.com/get-convex/convex-backend/blob/482df8a0e1288c9410be8241deb06b09d7ff70b0/crates/application/src/lib.rs)
