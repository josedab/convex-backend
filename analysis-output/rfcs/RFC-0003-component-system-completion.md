# RFC-0003: Component System Completion

**Status:** Draft
**Author:** Claude (AI Analysis)
**Created:** November 19, 2025
**Analysis Commit:** `482df8a0e1288c9410be8241deb06b09d7ff70b0`

---

## Summary

Complete the component system implementation by removing all `by_component_TODO` placeholders, adding HTTP action support in components, ensuring deterministic child seeding, and fully documenting the component architecture.

## Motivation

The Convex component system allows developers to build reusable, composable packages of functionality. However, the implementation is incomplete with several blocking issues.

### `by_component_TODO` Placeholders

**Location:** `crates/value/src/table_mapping.rs:53`

```rust
impl TableMapping {
    /// TODO: This is a placeholder that should be replaced with proper
    /// component namespace resolution
    pub const fn by_component_TODO() -> Self {
        Self {
            namespace: Namespace::Global,  // Wrong!
            // Should be Namespace::Component(component_id)
        }
    }
}
```

This placeholder is called in multiple critical locations:

**`crates/application/src/lib.rs:2586`**
```rust
// When executing functions in components
let table_mapping = TableMapping::by_component_TODO();
// Should be: TableMapping::for_component(component_id)
```

**`crates/application/src/lib.rs:3285`**
```rust
// When resolving table references
let mapping = TableMapping::by_component_TODO();
```

**`crates/application/src/application_function_runner/mod.rs:2138`**
```rust
// In function execution context
let ctx_mapping = TableMapping::by_component_TODO();
```

### HTTP Actions in Components

**Location:** `crates/application/src/function_log.rs:207`

```rust
pub fn log_http_action(
    &self,
    // ...
) {
    // TODO(ENG-7612): Support HTTP actions in components.
    // Currently HTTP actions can only run in the root component
    if component_id.is_some() {
        return Err(anyhow!("HTTP actions not supported in components"));
    }
}
```

And at line 1091:
```rust
// TODO(ENG-7612): Support HTTP actions in components.
```

### Component Child Seeding

**Location:** `crates/application/src/tests/components.rs:272-293`

```rust
#[test]
fn test_component_child_randomness() {
    // TODO: Fix this.
    // Currently child components get independent random seeds,
    // making their behavior non-deterministic relative to parent

    // TODO: the child's random seed should depend on the parent's, so the
    // entire component tree is deterministic from a single root seed
}
```

### Impact

1. **Feature Incomplete**: Users cannot build full-featured components
2. **Namespace Collisions**: Without proper scoping, tables in different components could conflict
3. **Non-determinism**: Child component behavior cannot be reproduced
4. **Code Smell**: Production code has TODO placeholders
5. **Adoption Blocker**: Teams cannot rely on components for production use

## Detailed Design

### Phase 1: Component Namespace Resolution

Replace `by_component_TODO()` with proper component context:

**New Types:**

```rust
// crates/value/src/namespace.rs

/// Identifies a component in the component tree
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ComponentId {
    /// Path from root: ["app", "auth", "oauth"]
    pub path: Vec<String>,
}

impl ComponentId {
    pub fn root() -> Self {
        Self { path: vec![] }
    }

    pub fn child(&self, name: &str) -> Self {
        let mut path = self.path.clone();
        path.push(name.to_string());
        Self { path }
    }

    pub fn is_root(&self) -> bool {
        self.path.is_empty()
    }
}

/// Namespace for table resolution
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Namespace {
    /// Tables in the root application
    Root,
    /// Tables scoped to a specific component
    Component(ComponentId),
}
```

**Updated TableMapping:**

```rust
// crates/value/src/table_mapping.rs

impl TableMapping {
    /// Create a table mapping for a specific component
    pub fn for_component(component_id: ComponentId) -> Self {
        Self {
            namespace: if component_id.is_root() {
                Namespace::Root
            } else {
                Namespace::Component(component_id)
            },
            tables: BTreeMap::new(),
        }
    }

    /// Resolve a table name to its fully-qualified ID
    pub fn resolve(&self, table_name: &str) -> Option<TableId> {
        // First check component-local tables
        if let Some(id) = self.tables.get(table_name) {
            return Some(*id);
        }

        // Then check exported tables from dependencies
        self.resolve_from_dependencies(table_name)
    }
}
```

**Update Call Sites:**

```rust
// crates/application/src/lib.rs

impl<RT: Runtime> Application<RT> {
    pub async fn execute_function(
        &self,
        component_id: ComponentId,  // NEW parameter
        path: FunctionPath,
        args: ConvexObject,
    ) -> anyhow::Result<FunctionResult> {
        // Create component-scoped table mapping
        let table_mapping = TableMapping::for_component(component_id.clone());

        // Create execution context with component scope
        let context = ExecutionContext {
            component_id,
            table_mapping,
            // ...
        };

        self.runner.execute(context, path, args).await
    }
}
```

### Phase 2: HTTP Actions in Components

Enable HTTP actions to execute within component contexts:

**Router Changes:**

```rust
// crates/local_backend/src/http_actions.rs

pub async fn http_action_handler(
    State(state): State<RouterState>,
    request: Request<Body>,
) -> Response<Body> {
    // Parse component path from URL
    // e.g., /component/auth/api/callback -> component_id="auth", path="api/callback"
    let (component_id, action_path) = parse_component_path(&request)?;

    let action_request = HttpActionRequest {
        method: request.method().clone(),
        url: request.uri().to_string(),
        headers: extract_headers(&request),
        body: extract_body(request).await?,
    };

    // Execute with component context
    let result = state.api
        .execute_http_action(
            &host,
            request_id,
            component_id,  // NEW: pass component context
            action_request,
            identity,
            caller,
            response_streamer,
        )
        .await;

    result.into_http_response()
}

fn parse_component_path(request: &Request<Body>) -> Result<(ComponentId, String)> {
    let path = request.uri().path();

    if path.starts_with("/component/") {
        // /component/{component_name}/{action_path}
        let parts: Vec<&str> = path.strip_prefix("/component/")
            .unwrap()
            .splitn(2, '/')
            .collect();

        let component_id = ComponentId::root().child(parts[0]);
        let action_path = parts.get(1).unwrap_or(&"").to_string();

        Ok((component_id, action_path))
    } else {
        // Root component
        Ok((ComponentId::root(), path.to_string()))
    }
}
```

**Application API Changes:**

```rust
// crates/application/src/api.rs

#[async_trait]
pub trait ApplicationApi: Send + Sync {
    async fn execute_http_action(
        &self,
        host: &ResolvedHostname,
        request_id: RequestId,
        component_id: ComponentId,  // NEW parameter
        http_request_metadata: HttpActionRequest,
        identity: Identity,
        caller: FunctionCaller,
        response_streamer: HttpActionResponseStreamer,
    ) -> anyhow::Result<()>;
}
```

**Function Log Support:**

```rust
// crates/application/src/function_log.rs

impl<RT: Runtime> FunctionExecutionLog<RT> {
    pub fn log_http_action(
        &self,
        component_id: &ComponentId,  // Now supported
        action_path: &str,
        request: &HttpActionRequest,
        response: &HttpActionResponse,
        duration: Duration,
    ) -> anyhow::Result<()> {
        let log_entry = HttpActionLogEntry {
            component: component_id.path.join("/"),
            action: action_path.to_string(),
            method: request.method.to_string(),
            status: response.status,
            duration_ms: duration.as_millis() as u64,
            timestamp: self.runtime.system_time(),
        };

        self.log_manager.write(log_entry)
    }
}
```

### Phase 3: Deterministic Child Seeding

Ensure child components have reproducible random behavior:

**Seed Derivation:**

```rust
// crates/application/src/component_context.rs

use std::hash::{Hash, Hasher};
use std::collections::hash_map::DefaultHasher;

impl ComponentContext {
    /// Derive a deterministic seed for a child component
    pub fn child_seed(&self, child_name: &str) -> u64 {
        let mut hasher = DefaultHasher::new();

        // Include parent seed
        hasher.write_u64(self.seed);

        // Include child name for uniqueness
        child_name.hash(&mut hasher);

        // Include a domain separator
        "convex-child-component".hash(&mut hasher);

        hasher.finish()
    }

    /// Create context for a child component
    pub fn child_context(&self, child_name: &str) -> Self {
        Self {
            component_id: self.component_id.child(child_name),
            seed: self.child_seed(child_name),
            parent: Some(Box::new(self.clone())),
        }
    }
}
```

**Usage in Function Runner:**

```rust
// crates/application/src/application_function_runner/mod.rs

impl<RT: Runtime> ApplicationFunctionRunner<RT> {
    async fn call_child_component(
        &self,
        parent_ctx: &ComponentContext,
        child_name: &str,
        function: &str,
        args: ConvexObject,
    ) -> anyhow::Result<ConvexValue> {
        // Derive deterministic child context
        let child_ctx = parent_ctx.child_context(child_name);

        // Execute in child context
        self.execute_in_context(child_ctx, function, args).await
    }
}
```

### Phase 4: Component Documentation

Create comprehensive documentation:

```rust
// crates/application/src/components/README.md

//! # Convex Component System
//!
//! ## Overview
//!
//! Components are reusable, composable packages of Convex functionality.
//! Each component has:
//! - Its own namespace for tables
//! - Isolated functions (queries, mutations, actions)
//! - HTTP action endpoints
//! - Deterministic behavior
//!
//! ## Component Tree
//!
//! ```
//! root (your app)
//! ├── auth (component)
//! │   ├── tables: users, sessions
//! │   └── functions: login, logout
//! └── payments (component)
//!     ├── tables: transactions
//!     └── functions: charge, refund
//! ```
//!
//! ## Table Namespacing
//!
//! Tables are scoped to their component:
//! - `root.users` - table in root app
//! - `auth.users` - table in auth component (different!)
//!
//! ## HTTP Actions
//!
//! HTTP actions are routed by component:
//! - `POST /api/action` - root component
//! - `POST /component/auth/callback` - auth component
```

## Example Usage

### Before

```typescript
// Developer tries to use component
import { component } from "convex/components";

const auth = component("@convex/auth");

// This fails because HTTP actions don't work in components
export const http = httpRouter();
http.route({
  path: "/auth/callback",
  method: "GET",
  handler: auth.httpAction("callback"),  // ERROR!
});
```

### After

```typescript
// Developer uses component with full functionality
import { component } from "convex/components";

const auth = component("@convex/auth");

// HTTP actions now work in components
export const http = httpRouter();
http.route({
  path: "/component/auth/callback",
  method: "GET",
  handler: auth.httpAction("callback"),  // Works!
});

// Tables are properly namespaced
export const getUser = query({
  handler: async (ctx) => {
    // Reads from auth component's users table
    return await auth.query(ctx, "getUser", { id: userId });
  },
});
```

## Implementation Plan

### Milestones

| Phase | Duration | Deliverable | Risk |
|-------|----------|-------------|------|
| 1. Namespace resolution | 4 days | Remove by_component_TODO | Medium - core change |
| 2. HTTP actions | 4 days | HTTP actions in components | Low - additive |
| 3. Deterministic seeding | 2 days | Reproducible child behavior | Low - isolated change |
| 4. Documentation | 2 days | Complete component docs | Low |

### Testing Strategy

1. **Unit Tests**
   - ComponentId path manipulation
   - Seed derivation determinism
   - Table resolution in namespaces

2. **Integration Tests**
   - Cross-component function calls
   - HTTP action routing to components
   - Nested component trees

3. **Property Tests**
   - Child seeds are deterministic
   - Table names don't collide across components

### Rollout Plan

1. **Week 1**: Internal testing with feature flag
2. **Week 2**: Beta users opt-in
3. **Week 3**: General availability
4. **Week 4**: Remove feature flag

## Backwards Compatibility

### Breaking Changes

None - this is additive functionality.

### Migration

Existing code continues to work:
- Apps without components are unaffected
- Current component usage will work better

### Deprecation

The `by_component_TODO()` function will be removed:
```rust
#[deprecated(note = "Use TableMapping::for_component() instead")]
pub const fn by_component_TODO() -> Self {
    Self::for_component(ComponentId::root())
}
```

## Alternatives Considered

### 1. Flat Namespace with Prefixes

Components use prefixed table names: `auth_users`, `auth_sessions`.

**Pros:** Simple implementation
**Cons:**
- Collision risk with user tables
- Ugly table names
- No true isolation

### 2. Virtual Tables

Components see virtual tables that map to real tables.

**Pros:** Complete isolation
**Cons:**
- Performance overhead
- Complexity
- Debugging difficulty

### 3. Separate Databases

Each component gets its own database.

**Pros:** Perfect isolation
**Cons:**
- Cannot join across components
- Massive overhead
- Operational complexity

The proposed namespace approach provides good isolation with minimal overhead.

## Open Questions

1. **Should components be able to access parent tables?**
   - Current design: No, explicit exports only
   - Alternative: Read access to parent namespace

2. **How should component versioning work?**
   - Need to handle schema migrations within components

3. **What about component-to-component dependencies?**
   - Circular dependencies are problematic

## Success Criteria

- [ ] 0 instances of `by_component_TODO` in codebase
- [ ] HTTP actions work in components (ENG-7612 resolved)
- [ ] Child component seeding is deterministic (same seed → same behavior)
- [ ] All component tests pass
- [ ] Documentation complete for component system
- [ ] No namespace collisions in test suite

## Effort Estimation

| Task | Effort | Owner |
|------|--------|-------|
| Phase 1: Namespace resolution | 4 dev-days | Backend |
| Phase 2: HTTP actions | 4 dev-days | Backend |
| Phase 3: Deterministic seeding | 2 dev-days | Backend |
| Phase 4: Documentation | 2 dev-days | Backend |
| Code review | 2 dev-days | Team |
| Testing & QA | 2 dev-days | QA |

**Total: ~2 weeks with 1 engineer**

## Stakeholders

- **Engineering**: Required - core implementation
- **Product**: Required - feature completion for launch
- **Documentation**: Required - user-facing docs
- **DevRel**: Interested - component examples and tutorials

---

*References:*
- [Table Mapping](https://github.com/get-convex/convex-backend/blob/482df8a0e1288c9410be8241deb06b09d7ff70b0/crates/value/src/table_mapping.rs)
- [Application](https://github.com/get-convex/convex-backend/blob/482df8a0e1288c9410be8241deb06b09d7ff70b0/crates/application/src/lib.rs)
- [Function Log](https://github.com/get-convex/convex-backend/blob/482df8a0e1288c9410be8241deb06b09d7ff70b0/crates/application/src/function_log.rs)
- [Component Tests](https://github.com/get-convex/convex-backend/blob/482df8a0e1288c9410be8241deb06b09d7ff70b0/crates/application/src/tests/components.rs)
