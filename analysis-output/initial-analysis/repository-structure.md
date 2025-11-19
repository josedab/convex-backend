# Repository Structure

**Analysis Commit:** `482df8a0e1288c9410be8241deb06b09d7ff70b0`

---

## Top-Level Directory

```
convex-backend/
├── crates/                      # Rust workspace (64 crates, 284K LOC)
├── npm-packages/                # TypeScript/JS packages (35+ packages, 311K LOC)
├── self-hosted/                 # Deployment configurations
├── demo/                        # Demo application
├── scripts/                     # Build and utility scripts
├── .github/                     # GitHub Actions CI/CD
├── Cargo.toml                   # Rust workspace configuration
├── Cargo.lock                   # Locked dependency versions
├── Justfile                     # Task runner commands
├── BUILD.md                     # Build instructions
├── CONTRIBUTING.md              # Contribution guidelines
├── README.md                    # Project documentation
├── LICENSE.md                   # MIT License
├── rust-toolchain               # Rust nightly specification
├── rustfmt.toml                 # Rust formatting config
├── dprint.json                  # Code formatting config
└── .nvmrc                       # Node.js version (20.19.5)
```

## Rust Crates Organization

### Core Infrastructure

| Crate | Lines of Code | Description |
|-------|--------------|-------------|
| `database` | 46,858 | MVCC database engine, snapshots, indexing |
| `common` | 39,159 | Shared utilities, HTTP, runtime abstraction |
| `isolate` | 33,916 | V8/Deno JavaScript execution environment |
| `application` | 28,468 | Business logic, UDF coordination |
| `model` | 20,332 | Data models, ORM-like abstractions |
| `local_backend` | 10,340 | HTTP server, API handlers |
| `value` | 9,269 | Convex value types and serialization |

### Search & Indexing

| Crate | Lines of Code | Description |
|-------|--------------|-------------|
| `search` | 13,637 | Full-text search (Tantivy-based) |
| `vector` | 5,007 | Vector embeddings and semantic search |
| `indexing` | 3,378 | Index management and maintenance |
| `text_search` | ~1,000 | Text analysis and stemming |

### Database Drivers

| Crate | Lines of Code | Description |
|-------|--------------|-------------|
| `postgres` | 4,551 | PostgreSQL adapter |
| `mysql` | 4,525 | MySQL adapter |
| `sqlite` | ~1,000 | SQLite integration (bundled) |
| `db_connection` | ~1,000 | Connection pooling |

### Networking & Communication

| Crate | Lines of Code | Description |
|-------|--------------|-------------|
| `convex` | 7,090 | Official Rust client library |
| `http_client` | ~1,000 | HTTP client utilities |
| `sync` | ~2,000 | Synchronization protocols |
| `events` | ~1,500 | Event streaming |

### External Integrations

| Crate | Lines of Code | Description |
|-------|--------------|-------------|
| `fivetran_destination` | 5,417 | Fivetran data export connector |
| `fivetran_source` | 3,349 | Fivetran data import connector |
| `fivetran_common` | ~1,000 | Shared Fivetran utilities |
| `aws_s3` | ~2,000 | AWS S3 storage integration |

### Security & Authentication

| Crate | Lines of Code | Description |
|-------|--------------|-------------|
| `authentication` | 1,290 | Auth token validation |
| `keybroker` | 2,051 | Key generation and management |
| `sodium_secretbox` | ~500 | NaCl encryption |

### Testing & Development

| Crate | Lines of Code | Description |
|-------|--------------|-------------|
| `simulation` | 2,819 | Deterministic testing framework |
| `load_generator` | 1,703 | Performance testing |
| `backend_harness` | ~1,000 | Test infrastructure |

### Utilities

| Crate | Description |
|-------|-------------|
| `errors` | Unified error types |
| `metrics` | Prometheus metrics |
| `convex_macro` | Procedural macros |
| `cmd_util` | CLI utilities |
| `config_loader` | Configuration management |
| `async_lru` | Async LRU cache |
| `interval_map` | Interval-based map |
| `packed_value` | Efficient binary encoding |
| `pb` | Protocol Buffer definitions |

## NPM Packages Organization

### Core Packages

| Package | Lines of Code | Description |
|---------|--------------|-------------|
| `convex` | 57,589 | Official JS/TS client SDK |
| `dashboard-common` | 51,998 | Shared UI components |
| `dashboard` | 44,140 | Cloud platform dashboard (Next.js) |
| `private-demos` | 42,502 | Internal demo applications |
| `demos` | 20,811 | Public demo projects |

### Runtime & Execution

| Package | Lines of Code | Description |
|---------|--------------|-------------|
| `udf-runtime` | 7,922 | UDF execution environment |
| `udf-tests` | 11,492 | UDF test suite |
| `system-udfs` | 5,711 | Built-in system functions |
| `node-executor` | 3,525 | Node.js executor |

### Testing & Tools

| Package | Lines of Code | Description |
|---------|--------------|-------------|
| `js-integration-tests` | 9,438 | Integration test suite |
| `local-store` | 6,446 | Local state management |
| `component-tests` | 3,618 | React component tests |
| `scenario-runner` | 2,755 | Test scenario framework |

## Key Directories Deep Dive

### `/crates/database/`
```
database/
├── src/
│   ├── lib.rs                    # Public API
│   ├── database.rs               # Core Database<RT> struct
│   ├── transaction.rs            # OCC transaction implementation
│   ├── snapshot_manager.rs       # MVCC snapshot management
│   ├── subscription.rs           # Query subscriptions
│   ├── committer.rs              # Transaction commit logic
│   ├── persistence/              # Storage backend abstraction
│   ├── text_index_worker/        # Full-text indexing
│   ├── vector_index_worker/      # Vector indexing
│   ├── tests/                    # Test suite
│   └── metrics.rs                # Prometheus metrics
├── benches/                      # Criterion benchmarks
└── README.md                     # Architecture documentation
```

### `/crates/application/`
```
application/
├── src/
│   ├── lib.rs                    # Application<RT> struct
│   ├── api.rs                    # ApplicationApi trait
│   ├── function_log.rs           # Execution logging
│   ├── application_function_runner/ # UDF execution
│   ├── cron_jobs/                # Scheduled job execution
│   ├── snapshot_import/          # Data import
│   ├── snapshot_export/          # Data export
│   ├── deploy_config/            # Deployment configuration
│   ├── schema_worker/            # Schema validation
│   └── tests/                    # 22 test modules
└── Cargo.toml
```

### `/crates/isolate/`
```
isolate/
├── src/
│   ├── lib.rs                    # Isolate public API
│   ├── isolate.rs                # V8 isolate wrapper
│   ├── execution_scope.rs        # JS execution context
│   ├── ops/                      # syscall implementations
│   ├── environment/              # Runtime environment
│   ├── bundled_js/               # Built-in JS modules
│   ├── client/                   # Isolate client
│   └── tests/                    # 35 test modules
├── benches/                      # Performance benchmarks
└── README.md                     # Serialization documentation
```

### `/crates/local_backend/`
```
local_backend/
├── src/
│   ├── main.rs                   # Entry point
│   ├── lib.rs                    # Library exports
│   ├── config.rs                 # LocalConfig struct
│   ├── router.rs                 # Axum router setup
│   ├── public_api/               # Public API handlers
│   ├── http_actions.rs           # HTTP action routing
│   ├── storage.rs                # File storage endpoints
│   ├── dashboard.rs              # Dashboard API
│   └── logs.rs                   # Log streaming
└── Cargo.toml
```

### `/self-hosted/`
```
self-hosted/
├── docker/
│   ├── docker-compose.yml        # Service composition
│   └── .env.example              # Environment template
├── docker-build/
│   └── Dockerfile.backend        # Multi-stage build
├── fly/                          # Fly.io deployment
├── railway/                      # Railway deployment
├── render/                       # Render deployment
├── aws-ecs/                      # AWS ECS deployment
└── README.md                     # Deployment guide
```

## Configuration Files

| File | Purpose |
|------|---------|
| `Cargo.toml` | Rust workspace definition |
| `rustfmt.toml` | Rust formatting rules |
| `.config/nextest.toml` | Test runner configuration |
| `dprint.json` | Multi-language formatting |
| `.prettierrc.js` | JavaScript formatting |
| `rust-toolchain` | Rust version pinning |

## GitHub Actions Workflows

| Workflow | Purpose |
|----------|---------|
| `build_local_backend.yml` | Main CI pipeline |
| `test_self_hosted_backend.yml` | E2E testing |
| `prettier.yml` | Format checking |

---

*Total repository size: 190 MB (including .git)*
