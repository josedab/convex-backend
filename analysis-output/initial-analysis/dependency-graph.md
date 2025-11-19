# Dependency Graph

**Analysis Commit:** `482df8a0e1288c9410be8241deb06b09d7ff70b0`

---

## Internal Crate Dependencies

### Core Dependency Flow

```
                    ┌─────────────┐
                    │ local_backend│
                    └──────┬──────┘
                           │
                    ┌──────▼──────┐
                    │ application │
                    └──────┬──────┘
                           │
          ┌────────────────┼────────────────┐
          │                │                │
    ┌─────▼─────┐    ┌─────▼─────┐    ┌─────▼─────┐
    │  isolate  │    │  database │    │   model   │
    └─────┬─────┘    └─────┬─────┘    └─────┬─────┘
          │                │                │
          └────────────────┼────────────────┘
                           │
                    ┌──────▼──────┐
                    │   common    │
                    └──────┬──────┘
                           │
                    ┌──────▼──────┐
                    │    value    │
                    └──────┬──────┘
                           │
                    ┌──────▼──────┐
                    │   errors    │
                    └─────────────┘
```

### Detailed Internal Dependencies

#### local_backend
```
local_backend
├── application
├── common
├── database
├── model
├── runtime
├── isolate
├── authentication
├── keybroker
├── metrics
├── storage
├── pb
└── value
```

#### application
```
application
├── database
├── model
├── common
├── runtime
├── isolate
├── search
├── vector
├── indexing
├── usage_tracking
├── function_log
├── file_storage
├── keybroker
├── authentication
├── events
└── value
```

#### database
```
database
├── common
├── model
├── runtime
├── value
├── errors
├── metrics
├── search
├── vector
├── indexing
├── pb
└── async_lru
```

#### isolate
```
isolate
├── common
├── runtime
├── value
├── errors
├── model
├── udf
├── deno_core
└── metrics
```

## External Dependency Categories

### Async Runtime Stack

```
┌─────────────────────────────────────────┐
│              Application Code           │
└──────────────────┬──────────────────────┘
                   │
┌──────────────────▼──────────────────────┐
│ tokio (1.47.1)                          │
│  ├── tokio-util (0.7.13)                │
│  ├── tokio-stream (0.1)                 │
│  └── tokio-metrics (0.4.0)              │
└──────────────────┬──────────────────────┘
                   │
┌──────────────────▼──────────────────────┐
│ futures (0.3)                           │
│  ├── futures-util (0.3.30)              │
│  └── futures-async-stream (0.2.11)      │
└─────────────────────────────────────────┘
```

### HTTP Stack

```
┌─────────────────────────────────────────┐
│              HTTP Handlers              │
└──────────────────┬──────────────────────┘
                   │
┌──────────────────▼──────────────────────┐
│ axum (0.8)                              │
│  ├── axum-extra (0.10)                  │
│  ├── tower (0.5.2)                      │
│  └── tower-http (0.6)                   │
└──────────────────┬──────────────────────┘
                   │
┌──────────────────▼──────────────────────┐
│ hyper (1.3.1)                           │
│  └── hyper-util (0.1.5)                 │
└──────────────────┬──────────────────────┘
                   │
┌──────────────────▼──────────────────────┐
│ http (1.0.0)                            │
│  └── http-body-util (0.1.2)             │
└─────────────────────────────────────────┘
```

### Serialization Stack

```
┌─────────────────────────────────────────┐
│         Data Serialization              │
└──────────────────┬──────────────────────┘
                   │
       ┌───────────┼───────────┐
       │           │           │
┌──────▼──────┐ ┌──▼───┐ ┌─────▼─────┐
│ serde (1.x) │ │prost │ │ bincode   │
│  serde_json │ │(0.13)│ │           │
│  serde_bytes│ │      │ │           │
└─────────────┘ └──────┘ └───────────┘
```

### Cryptography Stack

```
┌─────────────────────────────────────────┐
│              Security Layer             │
└──────────────────┬──────────────────────┘
                   │
       ┌───────────┼───────────┐
       │           │           │
┌──────▼──────┐ ┌──▼───────┐ ┌─▼─────────┐
│rustls (0.23)│ │openssl   │ │aws-lc-rs  │
│  rustls-    │ │(0.10.72) │ │(1.13)     │
│  native-    │ │          │ │           │
│  certs      │ │          │ │           │
└─────────────┘ └──────────┘ └───────────┘
                   │
       ┌───────────┼───────────┐
       │           │           │
┌──────▼──────┐ ┌──▼───┐ ┌─────▼─────┐
│ sha2 (0.10) │ │ rsa  │ │ p256/p384 │
│ sha1 (0.10) │ │(0.9.6)│ │ (0.13)    │
└─────────────┘ └──────┘ └───────────┘
```

### Database Stack

```
┌─────────────────────────────────────────┐
│            Database Layer               │
└──────────────────┬──────────────────────┘
                   │
       ┌───────────┼───────────┐
       │           │           │
┌──────▼────────┐ ┌▼─────────┐ ┌▼────────┐
│tokio-postgres │ │mysql_async│ │rusqlite │
│(0.7.13)       │ │(fork)     │ │(0.32)   │
│  postgres-    │ │           │ │         │
│  protocol     │ │           │ │         │
└───────────────┘ └───────────┘ └─────────┘
```

### Search Stack

```
┌─────────────────────────────────────────┐
│             Search Layer                │
└──────────────────┬──────────────────────┘
                   │
           ┌───────┴───────┐
           │               │
    ┌──────▼──────┐ ┌──────▼──────┐
    │  tantivy    │ │   qdrant    │
    │  (fork)     │ │   (fork)    │
    │             │ │             │
    │ Full-text   │ │   Vector    │
    │  search     │ │   search    │
    └─────────────┘ └─────────────┘
```

### Observability Stack

```
┌─────────────────────────────────────────┐
│          Observability Layer            │
└──────────────────┬──────────────────────┘
                   │
       ┌───────────┼───────────┐
       │           │           │
┌──────▼──────┐ ┌──▼───────┐ ┌─▼─────────┐
│tracing(0.1) │ │prometheus│ │sentry     │
│  tracing-   │ │(fork)    │ │(0.37)     │
│  subscriber │ │          │ │           │
└─────────────┘ └──────────┘ └───────────┘
```

## Version Pinning Strategy

### Exact Versions
Critical dependencies with exact version pinning:
- `tokio = "1.47.1"`
- `bytes = "1.6.0"`
- `uuid = "1.6"`

### Caret Versions (Default)
Most dependencies use caret notation:
- `serde = "1"` (allows 1.x.x)
- `axum = "0.8"` (allows 0.8.x)
- `clap = "^4.1.8"` (allows 4.1.8+)

### Git References
Custom forks pinned to specific commits:
```toml
[dependencies]
tantivy = { git = "...", rev = "c745b09..." }
qdrant = { git = "...", rev = "a5d1b7b..." }
mysql_async = { git = "...", rev = "90ff86d..." }
```

## Dependency Risk Matrix

### Low Risk
| Dependency | Reason |
|------------|--------|
| tokio | Actively maintained, stable API |
| serde | Industry standard, stable |
| axum | Tokio team maintained |
| rustls | Memory safe TLS |

### Medium Risk
| Dependency | Reason |
|------------|--------|
| Custom forks | Need to track upstream patches |
| base64 (0.13) | Outdated version |
| pyo3 | FFI boundaries require care |

### Monitoring Required
| Dependency | Action |
|------------|--------|
| tantivy fork | Monitor for security patches |
| qdrant fork | Monitor for security patches |
| mysql_async fork | Monitor for security patches |
| openidconnect fork | Monitor for security patches |

## Workspace Inheritance

Dependencies are centralized in workspace `Cargo.toml`:

```toml
[workspace.dependencies]
tokio = { version = "1.47.1", features = ["full", ...] }
serde = { version = "1", features = ["derive", "rc"] }
axum = { version = "0.8", features = [...] }
```

Crates inherit via:
```toml
[dependencies]
tokio.workspace = true
serde.workspace = true
```

## Build Profiles

### Release Profile
```toml
[profile.release]
opt-level = 3
overflow-checks = true
panic = "abort"
```

### Dev Profile Optimizations
Expensive deps compiled with optimization:
```toml
[profile.dev.package]
proptest = { opt-level = 3 }
levenshtein_automata = { opt-level = 3 }
regex-syntax = { opt-level = 3 }
tokio = { opt-level = 3 }
```

---

*For full dependency listings, see `Cargo.lock`*
