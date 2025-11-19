# Terminology Glossary

**Analysis Commit:** `482df8a0e1288c9410be8241deb06b09d7ff70b0`

---

## Core Concepts

### UDF (User-Defined Function)
JavaScript/TypeScript functions written by developers that run on the Convex backend. There are four types:
- **Query** - Read-only, cacheable, subscribable
- **Mutation** - Read-write, transactional
- **Action** - Side effects allowed (API calls, file I/O)
- **HTTP Action** - Custom HTTP endpoint handlers

### Transaction
An atomic unit of database operations with ACID guarantees. Convex uses OCC (Optimistic Concurrency Control) where transactions are validated at commit time rather than using locks.

### Snapshot
An immutable view of the database at a specific timestamp. Used for consistent reads within a transaction.

### Subscription
A persistent connection where clients receive real-time updates when query results change.

### Isolate
A V8 JavaScript execution environment that provides security isolation for running user code.

## Database Concepts

### OCC (Optimistic Concurrency Control)
A concurrency control method where transactions proceed without locks and are validated at commit time. If conflicts are detected, the transaction is retried.

### MVCC (Multi-Version Concurrency Control)
A technique where multiple versions of data are maintained, allowing readers to access consistent snapshots while writers create new versions.

### RepeatableTimestamp
A timestamp at which reads are guaranteed to be repeatable. Used for snapshot isolation.

### WriteTimestamp
The timestamp assigned to a committed transaction's writes.

### ReadSet
The set of all reads performed during a transaction, used for OCC conflict detection.

### TableRegistry
In-memory metadata about all tables in the database.

### IndexRegistry
In-memory metadata about all indexes in the database.

## Architecture Components

### Application<RT>
The main application container (where RT is the Runtime type parameter). Coordinates UDF execution, manages workers, and handles business logic.

### Database<RT>
The core database engine. Manages snapshots, transactions, subscriptions, and persistence.

### Committer
Component responsible for validating and committing transactions.

### SnapshotManager
Maintains multiple database snapshots at different timestamps for MVCC support.

### Persistence
The storage backend abstraction. Implementations exist for SQLite, PostgreSQL, and MySQL.

### PersistenceReader
Trait for reading from the persistence layer.

### PersistenceWriter
Trait for writing to the persistence layer.

## Runtime Concepts

### Runtime Trait
An abstraction over async runtime operations. Allows swapping production Tokio runtime for deterministic test runtime.

### ProdRuntime
Production runtime using Tokio for async operations.

### TestRuntime
Testing runtime with virtualized time for deterministic tests.

### SpawnHandle
Handle for a spawned async task, allowing shutdown and joining.

## Isolate Concepts

### V8
Google's JavaScript engine used by Chrome and Node.js. Convex uses it via the Deno runtime.

### Deno Core
The core runtime layer of Deno, used for executing JavaScript in isolates.

### syscall
A controlled interface between user JavaScript code and the Rust backend for operations like database reads/writes.

### ExecutionScope
The context in which JavaScript code executes within an isolate.

## Search & Indexing

### Tantivy
A full-text search engine library written in Rust, similar to Apache Lucene.

### Qdrant
A vector similarity search engine used for semantic search and embeddings.

### Memory Index
An in-memory index structure for fast lookups.

### FragmentedSegment
A search index segment stored across multiple files.

## Value Types

### ConvexValue
The core data type representing values that can be stored in Convex. Includes:
- Null, Boolean, Integer, Float
- String, Bytes
- Array, Object
- Id (document reference)

### DocumentId
A unique identifier for a document in a table.

### TableId
A unique identifier for a table.

### IndexId
A unique identifier for an index.

## Error Handling

### ErrorMetadata
Structured error information including error code, short message, and descriptive message.

### ErrorCode
Enumeration of error types (BadRequest, Conflict, NotFound, etc.) that map to HTTP status codes.

### RedactedJsError
A JavaScript error with sensitive information removed for safe external display.

## Observability

### Tracing
Structured logging using the `tracing` crate for debugging and monitoring.

### Metrics
Prometheus-format metrics for monitoring system health.

### Sentry
Error tracking service for production error monitoring.

### fastrace
A distributed tracing library (custom fork) for tracking requests across services.

## Deployment

### LocalConfig
Configuration for the local backend server, including database connection, ports, and storage settings.

### InstanceSecret
A secret key that identifies and secures a Convex instance.

### AdminKey
An authentication key for administrative operations.

## Integration

### Fivetran
A data integration platform. Convex provides source and destination connectors.

### S3
Amazon's object storage service, used for file storage and exports.

## Common Abbreviations

| Abbreviation | Meaning |
|-------------|---------|
| OCC | Optimistic Concurrency Control |
| MVCC | Multi-Version Concurrency Control |
| UDF | User-Defined Function |
| RT | Runtime (type parameter) |
| LOC | Lines of Code |
| PR | Pull Request |
| CI/CD | Continuous Integration/Deployment |
| E2E | End-to-End |
| FFI | Foreign Function Interface |
| JWT | JSON Web Token |
| OIDC | OpenID Connect |
| TLS | Transport Layer Security |

## File Patterns

### `mod.rs`
Main module file that re-exports and organizes a Rust module.

### `lib.rs`
Library crate entry point that defines the public API.

### `main.rs`
Binary crate entry point containing the `main` function.

### `tests.rs`
Test module containing unit tests, often conditional with `#[cfg(test)]`.

## Code Patterns

### `#[convex_macro::test_runtime]`
A procedural macro that sets up the async runtime for tests.

### `#[async_trait]`
Macro enabling async methods in traits.

### `DbFixtures`
Test fixtures for database state setup.

### `TestPersistence`
In-memory persistence implementation for testing.

---

*For more detailed explanations, see the blog series and architecture documentation.*
