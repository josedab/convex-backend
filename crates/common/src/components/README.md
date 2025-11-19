# Convex Component System

## Overview

Components are reusable, composable packages of Convex functionality. Each component has:
- Its own namespace for tables
- Isolated functions (queries, mutations, actions)
- HTTP action endpoints
- Deterministic behavior

## Component Tree

Components form a hierarchical tree structure:

```
root (your app)
├── auth (component)
│   ├── tables: users, sessions
│   └── functions: login, logout
└── payments (component)
    ├── tables: transactions
    └── functions: charge, refund
```

## Key Types

### ComponentId

Globally unique system-assigned ID for a component:

```rust
pub enum ComponentId {
    Root,
    Child(DeveloperDocumentId),
}
```

### ComponentPath

Hierarchical path to a component in the component tree (e.g., `["dashboard", "analytics"]`):

```rust
pub struct ComponentPath {
    path: Vec<ComponentName>,
}
```

### TableNamespace

Tables are scoped by component namespace:

```rust
pub enum TableNamespace {
    Global,  // Root component and system tables
    ByComponent(DeveloperDocumentId),  // Child component tables
}
```

## Table Namespacing

Tables are scoped to their component, preventing naming collisions:
- `root.users` - table in root app
- `auth.users` - table in auth component (different table!)

### Conversion

```rust
// ComponentId to TableNamespace
let namespace: TableNamespace = component_id.into();

// TableNamespace to ComponentId
let component_id: ComponentId = namespace.into();
```

## HTTP Actions in Components

HTTP actions can be routed to specific components:

```
POST /api/action           -> root component
POST /component/auth/callback -> auth component
```

The routing is handled by `route_http_action()` in `http_routing.rs`, which:
1. Parses the URL path to extract component path
2. Resolves the component path to a ComponentId
3. Executes the HTTP action in the component context

## Deterministic Child Seeding

Child components receive deterministic random seeds derived from their parent's seed:

```rust
use common::components::derive_child_seed;

let child_seed = derive_child_seed(&parent_seed, &child_name);
```

This ensures:
- Same parent seed + same child name = same child seed
- Different children get different seeds
- The entire component tree is deterministic from a single root seed

## Component State

Components can be in different states:

```rust
pub enum ComponentState {
    Active,    // Mounted and operational
    Unmounted, // Functions unavailable, tables read-only
}
```

## Integration Points

### Function Execution

When executing functions, the component context flows through:
1. `ValidatedPathAndArgs` contains the component path
2. `Isolate2SyscallProvider` receives the ComponentId
3. Table operations use the component's namespace

### Scheduler

Scheduled jobs track their component:
- `VirtualSchedulerModel` takes a `TableNamespace` parameter
- Jobs are scoped to their component's namespace

### Logging

Function execution logs include component information:
- `FunctionEventSource` has `component_path`
- HTTP actions log their component path

## TODOs and Future Work

1. **Full HTTP action component support**: Thread component_path through HttpActionOutcome
2. **Deterministic child seeding integration**: Use `derive_child_seed` when calling child components
3. **Component cancel_job**: Add component parameter to cancel scheduled jobs in child components
4. **Streaming export**: Re-enable for non-root components
5. **Component versioning**: Handle schema migrations within components

## Related Files

- `crates/common/src/components/` - Core component types
- `crates/value/src/table_mapping.rs` - Table namespace types
- `crates/isolate/src/isolate2/runner.rs` - Function execution with component context
- `crates/application/src/function_log.rs` - Function logging with component info
- `crates/application/src/application_function_runner/http_routing.rs` - HTTP action routing

## Usage Example

```typescript
// Developer uses component with full functionality
import { component } from "convex/components";

const auth = component("@convex/auth");

// HTTP actions in components
export const http = httpRouter();
http.route({
  path: "/component/auth/callback",
  method: "GET",
  handler: auth.httpAction("callback"),
});

// Tables are properly namespaced
export const getUser = query({
  handler: async (ctx) => {
    // Reads from auth component's users table
    return await auth.query(ctx, "getUser", { id: userId });
  },
});
```
