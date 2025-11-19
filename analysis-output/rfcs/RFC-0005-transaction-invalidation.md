# RFC-0005: Transaction Invalidation Fix

**Status:** Draft
**Author:** Claude (AI Analysis)
**Created:** November 19, 2025
**Analysis Commit:** `482df8a0e1288c9410be8241deb06b09d7ff70b0`

---

## Summary

Fix edge cases in transaction invalidation where concurrent schema changes (particularly search index deletion) fail to invalidate dependent transactions, potentially causing data inconsistency.

## Motivation

### Primary Issue: Search Index Deletion

**Location:** `crates/database/src/transaction_index.rs:274`

```rust
// TODO(ENG-9324): this has the side effect of failing to invalidate
// transactions if the search index is removed.
```

Also noted in tests at `crates/database/src/tests/randomized_search_tests.rs:1451`:
```rust
// TODO(ENG-9324): deleting the index *should* invalidate the transaction, but
// it doesn't because we skip recording the read of the index metadata.
```

### The HACK Workaround

**Location:** `crates/database/src/transaction_index.rs:267`

```rust
impl TransactionIndex {
    pub fn search(&mut self, index_id: IndexId, query: &Query) -> Result<Vec<Doc>> {
        // HACK: instead of using `self.require_enabled` we access the
        // `IndexRegistry` directly to fetch index info, which skips recording the
        // read of `index.id()` into our `TransactionReadSet`.
        //
        // This means if the index is deleted, we won't detect the conflict.
        let index_info = self.index_registry.get(index_id)?;

        // Perform search using index
        self.searcher.search(index_info, query)
    }
}
```

### What Should Happen

When Transaction A reads from a search index:
1. Transaction A records that it depends on index X
2. Transaction B deletes index X
3. Transaction B commits
4. Transaction A attempts to commit
5. **Expected**: Transaction A is invalidated (conflict detected)
6. **Actual**: Transaction A commits successfully with stale data

### Impact

1. **Data Inconsistency**: Queries may return results from a deleted index
2. **Silent Corruption**: No error when invariant is violated
3. **Hard to Debug**: Intermittent, timing-dependent failures
4. **Trust Erosion**: OCC guarantees are compromised

### Scope

This affects all operations that depend on schema metadata:
- Search index reads
- Vector index reads
- Table existence checks
- Index configuration reads

## Detailed Design

### Root Cause Analysis

The OCC validation checks:
1. Documents read haven't changed
2. Index ranges scanned haven't changed
3. **Missing**: Schema metadata hasn't changed

Schema metadata includes:
- Index existence
- Index configuration
- Table existence
- Table schema

### Solution: Track Schema Metadata Reads

**New ReadSet Fields:**

```rust
// crates/database/src/transaction.rs

pub struct ReadSet {
    // Existing fields
    documents: BTreeMap<DocumentId, Timestamp>,
    index_ranges: Vec<(IndexId, IndexRange)>,

    // NEW: Schema metadata reads
    index_metadata: BTreeSet<IndexId>,
    table_metadata: BTreeSet<TableId>,
}

impl ReadSet {
    /// Record that we read metadata about an index
    pub fn record_index_metadata_read(&mut self, index_id: IndexId) {
        self.index_metadata.insert(index_id);
    }

    /// Record that we read metadata about a table
    pub fn record_table_metadata_read(&mut self, table_id: TableId) {
        self.table_metadata.insert(table_id);
    }
}
```

**Updated TransactionIndex:**

```rust
// crates/database/src/transaction_index.rs

impl TransactionIndex {
    pub fn search(
        &mut self,
        index_id: IndexId,
        query: &Query
    ) -> Result<Vec<Doc>> {
        // Record that we depend on this index existing
        self.read_set.record_index_metadata_read(index_id);

        // Now get the index info (will fail if deleted)
        let index_info = self.index_registry.get(index_id)?;

        // Perform search
        self.searcher.search(index_info, query)
    }

    pub fn vector_search(
        &mut self,
        index_id: IndexId,
        vector: &[f32],
        limit: usize
    ) -> Result<Vec<(Doc, f32)>> {
        // Record dependency
        self.read_set.record_index_metadata_read(index_id);

        let index_info = self.index_registry.get(index_id)?;
        self.vector_searcher.search(index_info, vector, limit)
    }
}
```

**Updated OCC Validation:**

```rust
// crates/database/src/committer.rs

impl Committer {
    async fn validate_transaction(
        &self,
        tx: &Transaction,
        commit_ts: WriteTimestamp,
    ) -> Result<(), ConflictError> {
        // Existing validation
        self.validate_document_reads(tx, commit_ts).await?;
        self.validate_index_ranges(tx, commit_ts).await?;

        // NEW: Validate schema metadata
        self.validate_index_metadata(tx, commit_ts).await?;
        self.validate_table_metadata(tx, commit_ts).await?;

        Ok(())
    }

    async fn validate_index_metadata(
        &self,
        tx: &Transaction,
        commit_ts: WriteTimestamp,
    ) -> Result<(), ConflictError> {
        for index_id in tx.read_set.index_metadata.iter() {
            // Check if index still exists at commit time
            let exists_now = self.index_registry
                .exists_at(*index_id, commit_ts);

            let existed_at_read = self.index_registry
                .exists_at(*index_id, tx.begin_timestamp);

            if existed_at_read && !exists_now {
                return Err(ConflictError::IndexDeleted {
                    index_id: *index_id,
                    deleted_at: self.index_registry.deletion_time(*index_id),
                });
            }

            // Check if index configuration changed
            let config_changed = self.index_registry
                .config_changed_between(
                    *index_id,
                    tx.begin_timestamp,
                    commit_ts
                );

            if config_changed {
                return Err(ConflictError::IndexConfigChanged {
                    index_id: *index_id,
                });
            }
        }

        Ok(())
    }

    async fn validate_table_metadata(
        &self,
        tx: &Transaction,
        commit_ts: WriteTimestamp,
    ) -> Result<(), ConflictError> {
        for table_id in tx.read_set.table_metadata.iter() {
            let exists_now = self.table_registry
                .exists_at(*table_id, commit_ts);

            let existed_at_read = self.table_registry
                .exists_at(*table_id, tx.begin_timestamp);

            if existed_at_read && !exists_now {
                return Err(ConflictError::TableDeleted {
                    table_id: *table_id,
                });
            }
        }

        Ok(())
    }
}
```

**New Error Types:**

```rust
// crates/database/src/errors.rs

pub enum ConflictError {
    // Existing variants
    DocumentModified { document_id: DocumentId, ... },
    WriteConflict { document_id: DocumentId },

    // NEW: Schema conflicts
    IndexDeleted {
        index_id: IndexId,
        deleted_at: Timestamp,
    },
    IndexConfigChanged {
        index_id: IndexId,
    },
    TableDeleted {
        table_id: TableId,
    },
}
```

### Index Registry Changes

Track deletion times for proper validation:

```rust
// crates/database/src/index_registry.rs

pub struct IndexRegistry {
    indexes: BTreeMap<IndexId, IndexInfo>,
    // NEW: Track when indexes were deleted
    deletions: BTreeMap<IndexId, Timestamp>,
}

impl IndexRegistry {
    pub fn delete(&mut self, index_id: IndexId, ts: Timestamp) {
        self.indexes.remove(&index_id);
        self.deletions.insert(index_id, ts);
    }

    pub fn exists_at(&self, index_id: IndexId, ts: Timestamp) -> bool {
        if let Some(deletion_ts) = self.deletions.get(&index_id) {
            ts < *deletion_ts
        } else {
            self.indexes.contains_key(&index_id)
        }
    }

    pub fn deletion_time(&self, index_id: IndexId) -> Option<Timestamp> {
        self.deletions.get(&index_id).copied()
    }
}
```

## Example Usage

### Before (Bug)

```rust
// Transaction A: Search using index
let tx_a = db.begin().await?;
let results = tx_a.search(search_index_id, "query").await?;
// tx_a does NOT record dependency on search_index_id

// Transaction B: Delete the index
let tx_b = db.begin().await?;
tx_b.delete_index(search_index_id).await?;
tx_b.commit().await?;  // Success

// Transaction A: Commit
tx_a.commit().await?;  // SUCCESS - but should fail!
// tx_a committed with results from a deleted index
```

### After (Fixed)

```rust
// Transaction A: Search using index
let tx_a = db.begin().await?;
let results = tx_a.search(search_index_id, "query").await?;
// tx_a records: read_set.index_metadata.insert(search_index_id)

// Transaction B: Delete the index
let tx_b = db.begin().await?;
tx_b.delete_index(search_index_id).await?;
tx_b.commit().await?;  // Success

// Transaction A: Commit
tx_a.commit().await?;  // ERROR: ConflictError::IndexDeleted
// Transaction properly invalidated
```

## Implementation Plan

### Milestones

| Phase | Duration | Deliverable |
|-------|----------|-------------|
| 1. ReadSet extension | 2 days | Track metadata reads |
| 2. Registry changes | 1 day | Track deletion times |
| 3. Validation logic | 2 days | Check metadata in OCC |
| 4. Error handling | 1 day | New conflict types |
| 5. Testing | 2 days | Comprehensive tests |

### Testing Strategy

1. **Unit Tests**
   - Index deletion invalidates reader
   - Table deletion invalidates reader
   - Config change invalidates reader
   - Non-conflicting changes allowed

2. **Integration Tests**
   - Concurrent index deletion and search
   - Concurrent table drop and query
   - Nested transactions with schema changes

3. **Property Tests**
   - All schema reads tracked
   - All conflicts detected
   - No false positives

4. **Regression Tests**
   - ENG-9324 specific scenario
   - Randomized search tests pass

### Files to Modify

- `crates/database/src/transaction.rs` - ReadSet extension
- `crates/database/src/transaction_index.rs` - Record metadata reads
- `crates/database/src/committer.rs` - Validate metadata
- `crates/database/src/index_registry.rs` - Track deletions
- `crates/database/src/table_registry.rs` - Track deletions
- `crates/database/src/errors.rs` - New error types

## Backwards Compatibility

- **No API changes**: Internal transaction validation
- **Stricter validation**: Some previously-successful commits will now fail
- **Correct behavior**: Failures indicate real conflicts

### Migration

No migration needed. Existing code will:
- Pass: If no schema conflicts
- Fail with retry: If schema conflicts (correct behavior)

## Alternatives Considered

### 1. Lock Schema During Transaction

Acquire lock on schema metadata during transaction.

**Pros:** Simple to implement
**Cons:**
- Blocks concurrent schema operations
- Reduces throughput
- Violates OCC philosophy

### 2. Snapshot Schema with Transaction

Include full schema snapshot in transaction.

**Pros:** Complete isolation
**Cons:**
- Memory overhead
- Doesn't catch all conflicts
- Complex implementation

### 3. Version Vectors for Schema

Use version vectors to track schema changes.

**Pros:** Fine-grained conflict detection
**Cons:**
- Complexity
- Storage overhead
- Overkill for this use case

The proposed approach extends the existing OCC mechanism naturally.

## Open Questions

1. **Should we validate all schema reads or just mutations?**
   - Recommendation: All reads, for consistency

2. **How long to retain deletion history?**
   - Recommendation: Until retention cleanup

3. **Should we distinguish index types?**
   - Recommendation: No, treat all indexes uniformly

## Success Criteria

- [ ] ENG-9324 resolved (index deletion invalidates transactions)
- [ ] HACK comment removed from transaction_index.rs
- [ ] Randomized search tests pass reliably
- [ ] No false positive invalidations
- [ ] Property tests for all schema operations
- [ ] Performance regression < 1%

## Effort Estimation

| Task | Effort |
|------|--------|
| ReadSet extension | 1 dev-day |
| Registry changes | 0.5 dev-days |
| Validation logic | 1.5 dev-days |
| Error handling | 0.5 dev-days |
| Testing | 1.5 dev-days |

**Total: ~1 week with 1 engineer**

## Stakeholders

- **Engineering**: Required - core correctness fix
- **Security**: Interested - data consistency guarantee

---

*References:*
- [Transaction Index](https://github.com/get-convex/convex-backend/blob/482df8a0e1288c9410be8241deb06b09d7ff70b0/crates/database/src/transaction_index.rs)
- [Committer](https://github.com/get-convex/convex-backend/blob/482df8a0e1288c9410be8241deb06b09d7ff70b0/crates/database/src/committer.rs)
- [Randomized Search Tests](https://github.com/get-convex/convex-backend/blob/482df8a0e1288c9410be8241deb06b09d7ff70b0/crates/database/src/tests/randomized_search_tests.rs)
