# RFC-0005: Transaction Invalidation Fix

**Status:** Draft
**Author:** Claude (AI Analysis)
**Created:** November 19, 2025
**Analysis Commit:** `482df8a0e1288c9410be8241deb06b09d7ff70b0`

---

## Summary

Fix edge cases in transaction invalidation, particularly when search indexes are deleted, to ensure correct OCC behavior.

## Motivation

### Search Index Deletion Issue

**Location:** `crates/database/src/transaction_index.rs:274`
```rust
// TODO(ENG-9324): this has the side effect of failing to invalidate
// transactions if the search index is removed.
```

Also at `crates/database/src/tests/randomized_search_tests.rs:1451`:
```rust
// TODO(ENG-9324): deleting the index *should* invalidate the transaction, but
```

### TransactionReadSet HACK

**Location:** `crates/database/src/transaction_index.rs:267`
```rust
// HACK: instead of using `self.require_enabled` we access the
// `IndexRegistry` directly to fetch index info, which skips recording the
// read of `index.id()` into our `TransactionReadSet`.
```

### Impact

- **Data consistency risk**: Transactions may commit with stale data
- **Silent failures**: No error when invariant is violated
- **Test flakiness**: Non-deterministic test behavior

## Detailed Design

### Root Cause

When reading from a search index:
1. Transaction records index read
2. Index is deleted concurrently
3. Commit validation doesn't detect the deletion
4. Transaction commits with invalid data

### Solution

Track index metadata reads in transaction:

```rust
impl TransactionIndex {
    pub fn search(&mut self, index_id: IndexId, query: &Query) -> Result<Vec<Doc>> {
        // Record that we read this index's existence
        self.read_set.record_index_metadata_read(index_id);

        // Perform the search
        self.searcher.search(index_id, query)
    }
}

impl Committer {
    fn validate(&self, tx: &Transaction) -> Result<()> {
        // Check index metadata reads
        for index_id in tx.read_set.index_metadata_reads() {
            if !self.index_registry.exists(index_id) {
                return Err(ConflictError::IndexDeleted { index_id });
            }
        }
        // ... other validation
    }
}
```

## Success Criteria

- [ ] Index deletion invalidates dependent transactions
- [ ] HACK comment removed
- [ ] Property tests for all invalidation cases
- [ ] No false positives (unnecessary invalidation)

## Effort Estimation

**Total: 1 week**

---

*References:*
- [Transaction Index](https://github.com/get-convex/convex-backend/blob/482df8a0e1288c9410be8241deb06b09d7ff70b0/crates/database/src/transaction_index.rs)
