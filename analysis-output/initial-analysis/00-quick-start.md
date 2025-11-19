# Convex Backend: Quick Start Guide

**Analysis Commit:** `482df8a0e1288c9410be8241deb06b09d7ff70b0`

---

## What is Convex?

Convex is a **reactive database platform** that automatically synchronizes data between your backend and frontend in real-time. Think of it as a database that:

1. **Pushes updates** to connected clients automatically
2. **Executes your code** in a secure JavaScript sandbox
3. **Guarantees consistency** with ACID transactions
4. **Scales horizontally** for production workloads

## Architecture at a Glance

```
┌─────────────────────────────────────────────────┐
│           Client Applications                    │
│      (React, Vue, Node.js with Convex SDK)      │
└────────────────────┬────────────────────────────┘
                     │ HTTP/WebSocket
                     ▼
┌─────────────────────────────────────────────────┐
│              Convex Backend                      │
│  ┌─────────────────────────────────────────┐    │
│  │   HTTP API (Axum)                       │    │
│  │   • Query/Mutation endpoints            │    │
│  │   • WebSocket subscriptions             │    │
│  └─────────────────────────────────────────┘    │
│  ┌─────────────────────────────────────────┐    │
│  │   Application Layer                     │    │
│  │   • UDF execution coordination          │    │
│  │   • Transaction management              │    │
│  └─────────────────────────────────────────┘    │
│  ┌─────────────────────────────────────────┐    │
│  │   Isolate (V8/Deno)                     │    │
│  │   • Secure JS execution                 │    │
│  │   • syscall interface                   │    │
│  └─────────────────────────────────────────┘    │
│  ┌─────────────────────────────────────────┐    │
│  │   Database Engine                       │    │
│  │   • MVCC snapshots                      │    │
│  │   • OCC transactions                    │    │
│  │   • Full-text & vector search           │    │
│  └─────────────────────────────────────────┘    │
└─────────────────────────────────────────────────┘
                     │
        ┌────────────┼────────────┐
        ▼            ▼            ▼
     SQLite     PostgreSQL     MySQL
```

## Key Concepts

### User-Defined Functions (UDFs)
Your application logic runs as JavaScript/TypeScript functions:
- **Queries** - Read-only, cached, subscribable
- **Mutations** - Read-write, transactional
- **Actions** - Side effects (API calls, file I/O)
- **HTTP Actions** - Custom HTTP endpoints

### Transactions
Every mutation runs in a transaction with:
- **Snapshot isolation** - Consistent reads
- **OCC validation** - No locks, retry on conflict
- **Automatic retries** - Framework handles conflicts

### Subscriptions
Clients subscribe to queries and receive:
- **Initial result** - Current data snapshot
- **Delta updates** - Only changed data
- **Automatic invalidation** - When underlying data changes

## Repository Structure

```
convex-backend/
├── crates/                    # 64 Rust crates
│   ├── database/              # Core database engine (46K LOC)
│   ├── application/           # Business logic (28K LOC)
│   ├── isolate/               # V8/Deno runtime (34K LOC)
│   ├── common/                # Shared utilities (39K LOC)
│   ├── local_backend/         # HTTP server (10K LOC)
│   └── ...                    # 59 more crates
├── npm-packages/              # TypeScript/JavaScript
│   ├── convex/                # Client SDK
│   ├── dashboard/             # Web dashboard
│   └── ...                    # 33 more packages
├── self-hosted/               # Deployment configs
└── Cargo.toml                 # Workspace root
```

## Top 5 Things to Know

### 1. Everything is a Transaction
Every read and write goes through the transaction system. This guarantees consistency but means you need to understand OCC conflicts.

**Key file:** `crates/database/src/transaction.rs`

### 2. JavaScript Runs in Isolates
User code executes in V8 isolates with restricted syscalls. The runtime serializes data between Rust and JavaScript.

**Key file:** `crates/isolate/src/lib.rs`

### 3. Subscriptions Drive Real-time
The subscription system tracks query dependencies and pushes updates when underlying data changes.

**Key file:** `crates/database/src/subscription.rs`

### 4. Persistence is Pluggable
The backend supports SQLite (default), PostgreSQL, and MySQL through a trait-based abstraction.

**Key file:** `crates/common/src/persistence.rs`

### 5. Metrics are Everywhere
Prometheus metrics are collected throughout the codebase for observability.

**Key file:** `crates/metrics/src/metrics.rs`

## Getting Started

### Building from Source
```bash
# Install Rust nightly
rustup toolchain install nightly-2025-06-28

# Install Node.js dependencies
just rush install

# Build the backend
cargo build --release

# Run locally
just run-local-backend
```

### Running with Docker
```bash
cd self-hosted/docker
docker-compose up
```

### Key Ports
- **3210** - HTTP API
- **3211** - HTTP Actions

## What to Read Next

1. **Architecture Overview** - `blog-series/01-architecture-overview.md`
2. **Database Deep Dive** - `blog-series/02-deep-dive-database.md`
3. **Repository Structure** - `initial-analysis/repository-structure.md`
4. **Improvement RFCs** - `rfcs/00-prioritization-matrix.md`

## Quick Reference

| Component | Location | Purpose |
|-----------|----------|---------|
| HTTP Router | `crates/local_backend/src/router.rs` | API endpoints |
| Application | `crates/application/src/lib.rs` | Business logic |
| Database | `crates/database/src/database.rs` | Data storage |
| Transaction | `crates/database/src/transaction.rs` | ACID guarantees |
| Isolate | `crates/isolate/src/isolate.rs` | JS execution |
| Persistence | `crates/common/src/persistence.rs` | Storage backend |

---

*This analysis is based on commit `482df8a0e1288c9410be8241deb06b09d7ff70b0`*
