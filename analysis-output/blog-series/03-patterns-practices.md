# Patterns and Practices in Convex

**Part 3 of the Convex Backend Technical Series**

**Analysis Commit:** [`482df8a`](https://github.com/get-convex/convex-backend/tree/482df8a0e1288c9410be8241deb06b09d7ff70b0)

---

## What You'll Learn

- The Runtime trait pattern for testable async code
- Structured error handling with ErrorMetadata
- Trait-based polymorphism throughout the codebase
- Testing strategies: unit, integration, and simulation
- Async patterns and background worker management

---

## Introduction

Good code isn't just about what it does—it's about how it's organized. In this post, we'll examine the design patterns that make Convex maintainable, testable, and extensible. These patterns have been refined through real production use and offer valuable lessons for any Rust project.

## Pattern 1: The Runtime Trait

Perhaps the most impactful pattern in Convex is the `Runtime` trait. It abstracts over system operations that are hard to test: time, randomness, and task spawning.

### The Problem

Consider testing a timeout:

```rust
// Hard to test
async fn with_timeout(duration: Duration) -> Result<Data, Error> {
    tokio::time::sleep(duration).await;
    // How do you test this without waiting?
}
```

### The Solution

```rust
// Source: crates/common/src/runtime/mod.rs (lines 285-349)
// https://github.com/get-convex/convex-backend/blob/482df8a0e1288c9410be8241deb06b09d7ff70b0/crates/common/src/runtime/mod.rs#L285-L349

#[async_trait]
pub trait Runtime: Clone + Sync + Send + 'static {
    fn wait(&self, duration: Duration)
        -> Pin<Box<dyn FusedFuture<Output = ()> + Send + 'static>>;

    fn system_time(&self) -> SystemTime;

    fn rng(&self) -> Box<dyn RngCore>;

    fn spawn(
        &self,
        name: &'static str,
        f: impl Future<Output = ()> + Send + 'static,
    ) -> Box<dyn SpawnHandle>;
}
```

Now components are parameterized by runtime:

```rust
pub struct Database<RT: Runtime> {
    runtime: RT,
    // ...
}

pub struct Application<RT: Runtime> {
    database: Database<RT>,
    runtime: RT,
    // ...
}
```

### Test Runtime Implementation

```rust
// TestRuntime can advance time instantly
impl Runtime for TestRuntime {
    fn wait(&self, duration: Duration)
        -> Pin<Box<dyn FusedFuture<Output = ()> + Send + 'static>> {
        // Don't actually sleep - just record the request
        self.advance_time(duration);
        Box::pin(futures::future::ready(()))
    }

    fn rng(&self) -> Box<dyn RngCore> {
        // Deterministic RNG for reproducible tests
        Box::new(ChaCha8Rng::seed_from_u64(self.seed))
    }
}
```

### Benefits

1. **Tests run instantly** - No waiting for timeouts
2. **Deterministic behavior** - Same seed, same results
3. **Simulation testing** - Complex scenarios can be tested

### Usage Pattern

```rust
#[convex_macro::test_runtime]
async fn test_timeout_behavior(rt: TestRuntime) {
    let db = Database::new(rt.clone());

    // Test code that would normally sleep
    let result = db.with_retry().await;

    // Time advanced instantly
    assert!(result.is_ok());
}
```

## Pattern 2: Structured Error Handling

Convex uses a structured approach to errors that enables both debugging and metrics.

### Error Metadata

```rust
// Source: crates/errors/src/lib.rs (lines 15-85)
// https://github.com/get-convex/convex-backend/blob/482df8a0e1288c9410be8241deb06b09d7ff70b0/crates/errors/src/lib.rs#L15-L85

#[derive(thiserror::Error, Clone, Debug)]
#[error("{msg}")]
pub struct ErrorMetadata {
    /// HTTP-like error code
    pub code: ErrorCode,

    /// Short code for metrics (stable across message changes)
    pub short_msg: Cow<'static, str>,

    /// Developer-facing message (can change)
    pub msg: Cow<'static, str>,

    /// Upstream service that caused the error
    pub source: Option<String>,
}
```

### Error Code Enumeration

```rust
pub enum ErrorCode {
    BadRequest,              // 400
    Unauthenticated,         // 401
    Forbidden,               // 403
    NotFound,                // 404
    Conflict,                // 409
    RateLimited,             // 429
    OCC {                    // Special case with details
        table_name: Option<String>,
        document_id: Option<String>,
        write_source: WriteSource,
        is_system: bool,
    },
    OperationalInternalServerError,  // 500
}
```

### Usage Pattern

```rust
// Creating errors with context
fn validate_document(doc: &Document) -> anyhow::Result<()> {
    if doc.id.is_empty() {
        anyhow::bail!(ErrorMetadata::bad_request(
            "EmptyDocumentId",
            "Document ID cannot be empty"
        ));
    }
    Ok(())
}

// The short_msg is for metrics
// The full msg is for developers
```

### Benefits

1. **Metrics stability** - `short_msg` doesn't change when copy is updated
2. **HTTP mapping** - Error codes map directly to status codes
3. **Source tracking** - Know which service caused the error
4. **OCC details** - Rich information for conflict debugging

## Pattern 3: Trait-Based Polymorphism

Convex uses traits extensively for abstraction without runtime cost.

### Storage Trait

```rust
// Source: crates/storage/src/lib.rs (lines 147-200)
// https://github.com/get-convex/convex-backend/blob/482df8a0e1288c9410be8241deb06b09d7ff70b0/crates/storage/src/lib.rs#L147-L200

#[async_trait]
pub trait Storage: Send + Sync + Debug {
    async fn start_upload(&self) -> anyhow::Result<Box<BufferedUpload>>;

    async fn signed_url(
        &self,
        key: ObjectKey,
        expires_in: Duration
    ) -> anyhow::Result<String>;

    async fn get_object_attributes(
        &self,
        key: &ObjectKey
    ) -> anyhow::Result<Option<ObjectAttributes>>;
}
```

Implementations:
- `LocalDirStorage` - For development
- `S3Storage` - For production

### ApplicationApi Trait

```rust
// Source: crates/application/src/api.rs
// https://github.com/get-convex/convex-backend/blob/482df8a0e1288c9410be8241deb06b09d7ff70b0/crates/application/src/api.rs

#[async_trait]
pub trait ApplicationApi: Send + Sync {
    async fn execute_public_query(/* ... */) -> anyhow::Result<RedactedQueryReturn>;
    async fn execute_public_mutation(/* ... */) -> anyhow::Result</*...*/>;
    async fn execute_http_action(/* ... */) -> anyhow::Result<()>;
}
```

This allows the HTTP layer to work with any implementation:

```rust
pub struct RouterState {
    pub api: Arc<dyn ApplicationApi>,
    pub runtime: ProdRuntime,
}
```

### When to Use Dynamic Dispatch

Convex uses `Arc<dyn Trait>` when:
- Multiple implementations exist at runtime
- Plugin/extension points
- External system boundaries

And uses generics `<T: Trait>` when:
- Single implementation per compilation
- Performance-critical paths
- Internal abstractions

## Pattern 4: Background Worker Management

Long-running background tasks need careful lifecycle management.

### SpawnHandle Pattern

```rust
// Source: crates/common/src/runtime/mod.rs (lines 108-165)
// https://github.com/get-convex/convex-backend/blob/482df8a0e1288c9410be8241deb06b09d7ff70b0/crates/common/src/runtime/mod.rs#L108-L165

pub trait SpawnHandle: Send + Sync {
    /// Signal the task to shut down
    fn shutdown(&mut self);

    /// Wait for the task to complete
    fn join(self) -> impl Future<Output = Result<(), JoinError>>;

    /// Let the task run independently
    fn detach(self: Box<Self>);
}
```

### Application Workers

```rust
// Source: crates/application/src/lib.rs (lines 544-571)
// https://github.com/get-convex/convex-backend/blob/482df8a0e1288c9410be8241deb06b09d7ff70b0/crates/application/src/lib.rs#L544-L571

pub struct Application<RT: Runtime> {
    // Workers are wrapped in Arc<Mutex<Option<_>>> for lifecycle
    scheduled_job_runner: ScheduledJobRunner,
    cron_job_executor: Arc<Mutex<Box<dyn SpawnHandle>>>,
    index_worker: Arc<Mutex<Option<Box<dyn SpawnHandle>>>>,
    search_worker: Arc<Mutex<SearchIndexWorkers>>,
    export_worker: Arc<Mutex<Option<Box<dyn SpawnHandle>>>>,
}
```

### Graceful Shutdown

```rust
// Source: crates/common/src/shutdown.rs
// https://github.com/get-convex/convex-backend/blob/482df8a0e1288c9410be8241deb06b09d7ff70b0/crates/common/src/shutdown.rs

pub struct ShutdownSignal {
    sender: broadcast::Sender<()>,
}

impl ShutdownSignal {
    pub fn signal(&self) {
        let _ = self.sender.send(());
    }
}

// Workers check for shutdown
async fn worker_loop(shutdown: ShutdownReceiver) {
    loop {
        tokio::select! {
            _ = shutdown.recv() => break,
            _ = do_work() => continue,
        }
    }
}
```

## Pattern 5: Testing Strategies

Convex employs multiple testing approaches for comprehensive coverage.

### Unit Tests with Test Macros

```rust
#[convex_macro::test_runtime]
async fn test_document_insert(rt: TestRuntime) -> anyhow::Result<()> {
    let db = Database::new(rt);
    let tx = db.begin().await?;

    let doc_id = tx.insert("users", json!({
        "name": "Alice"
    })).await?;

    tx.commit().await?;
    assert!(doc_id.is_valid());
    Ok(())
}
```

### Property-Based Testing

```rust
// Using proptest for invariant checking
use proptest::prelude::*;

proptest! {
    #[test]
    fn roundtrip_serialization(value in any::<ConvexValue>()) {
        let serialized = value.to_bytes();
        let deserialized = ConvexValue::from_bytes(&serialized)?;
        prop_assert_eq!(value, deserialized);
    }
}
```

### Integration Tests with Fixtures

```rust
// Source: crates/application/src/test_helpers.rs
// https://github.com/get-convex/convex-backend/blob/482df8a0e1288c9410be8241deb06b09d7ff70b0/crates/application/src/test_helpers.rs

pub struct DbFixtures<RT: Runtime> {
    pub db: Database<RT>,
    pub tp: Arc<dyn Persistence>,
}

impl<RT: Runtime> DbFixtures<RT> {
    pub async fn new(rt: RT) -> anyhow::Result<Self> {
        let tp = TestPersistence::new();
        let db = Database::load(tp.clone(), rt).await?;
        Ok(Self { db, tp })
    }
}
```

### Simulation Testing

For complex distributed scenarios:

```rust
// Source: crates/simulation/src/lib.rs
// https://github.com/get-convex/convex-backend/blob/482df8a0e1288c9410be8241deb06b09d7ff70b0/crates/simulation/src/lib.rs

pub struct Simulation {
    runtime: SimulationRuntime,
    nodes: Vec<SimulatedNode>,
}

impl Simulation {
    pub fn run_scenario(&mut self, scenario: Scenario) {
        // Deterministically execute events
        // Check invariants after each step
        // Verify consistency properties
    }
}
```

## Pattern 6: Configuration Loading

Hot-reloadable configuration with signal handling:

```rust
// Source: crates/config_loader/src/lib.rs (lines 34-99)
// https://github.com/get-convex/convex-backend/blob/482df8a0e1288c9410be8241deb06b09d7ff70b0/crates/config_loader/src/lib.rs#L34-L99

pub struct ConfigLoader<D: ConfigDecoder + Send + 'static> {
    config_rx: watch::Receiver<D::Output>,
    reload_tx: mpsc::Sender<()>,
    handle: Box<dyn SpawnHandle>,
}

impl<D: ConfigDecoder> ConfigLoader<D> {
    pub async fn new<RT: Runtime>(
        rt: RT,
        signal_kind: SignalKind,  // e.g., SIGHUP
        config_path: PathBuf,
        decoder: D,
    ) -> anyhow::Result<Self> {
        // Watch for signal and reload config
    }

    pub fn current(&self) -> D::Output {
        self.config_rx.borrow().clone()
    }
}
```

## Pattern 7: Metrics Collection

Compile-time validated metrics:

```rust
// Source: crates/metrics/src/metrics.rs
// https://github.com/get-convex/convex-backend/blob/482df8a0e1288c9410be8241deb06b09d7ff70b0/crates/metrics/src/metrics.rs

// Metrics must have unit suffixes
register_convex_histogram!(
    HTTP_HANDLE_DURATION_SECONDS,
    "Time to handle HTTP request",
    &["endpoint", "method", "status"]
);

// Usage
HTTP_HANDLE_DURATION_SECONDS
    .with_label_values(&["/api/query", "POST", "200"])
    .observe(duration.as_secs_f64());
```

Benefits:
- **Naming enforcement** - Units in metric names
- **Type safety** - Labels checked at compile time
- **Consistency** - Centralized metric definitions

## Code Organization Practices

### Module Structure

```
crate_name/
├── src/
│   ├── lib.rs          # Public API, re-exports
│   ├── module.rs       # Main implementation
│   ├── module/
│   │   ├── mod.rs      # Module organization
│   │   ├── types.rs    # Type definitions
│   │   └── tests.rs    # Module tests
│   └── tests/          # Integration tests
└── benches/            # Benchmarks
```

### Visibility Guidelines

- `pub` - Part of crate's public API
- `pub(crate)` - Internal to crate
- `pub(super)` - Internal to parent module
- Private - Default for implementation details

## Anti-Patterns to Avoid

### 1. Excessive Dynamic Dispatch

```rust
// Avoid: 600 instances in codebase
let handler: Box<dyn Handler> = ...;

// Prefer: Generic when possible
fn process<H: Handler>(handler: H) { ... }
```

### 2. Arc<Mutex> Everywhere

```rust
// Current pattern (causes contention)
pub struct UsageTracker {
    counters: Arc<Mutex<Counters>>,
}

// TODO: Move accounting out of Committer
// https://github.com/get-convex/convex-backend/blob/482df8a0e1288c9410be8241deb06b09d7ff70b0/crates/usage_tracking/src/lib.rs#L571-L576
```

### 3. Suppressing Warnings

```rust
// 78 instances of this
#[allow(dead_code)]
fn unused_function() { ... }

// Better: Remove or document why it's needed
```

## Key Takeaways

1. **Runtime trait enables testing** - Abstract system operations for determinism

2. **Structured errors for metrics** - Short codes survive copy changes

3. **Traits for abstraction** - Polymorphism without performance cost

4. **Workers need lifecycle** - SpawnHandle pattern for cleanup

5. **Multiple test levels** - Unit, property, integration, simulation

## What's Next?

In the next post, we'll explore how Convex's subscription system delivers real-time updates to clients efficiently.

---

## References

- [Runtime Implementation](https://github.com/get-convex/convex-backend/blob/482df8a0e1288c9410be8241deb06b09d7ff70b0/crates/common/src/runtime/mod.rs)
- [Error Types](https://github.com/get-convex/convex-backend/blob/482df8a0e1288c9410be8241deb06b09d7ff70b0/crates/errors/src/lib.rs)
- [Test Helpers](https://github.com/get-convex/convex-backend/blob/482df8a0e1288c9410be8241deb06b09d7ff70b0/crates/application/src/test_helpers.rs)

---

*Next: [Part 4 - Real-time Magic: How Convex Subscriptions Work](04-realtime-subscriptions.md)*
