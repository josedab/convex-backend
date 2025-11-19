# RFC-0001: Search + Filter Performance Optimization

**Status:** Draft
**Author:** Claude (AI Analysis)
**Created:** November 19, 2025
**Analysis Commit:** `482df8a0e1288c9410be8241deb06b09d7ff70b0`

---

## Summary

Optimize the performance of combined text search and filter operations by pushing filter predicates into the search index evaluation, eliminating the current two-phase approach that causes redundant document loading.

## Motivation

The current implementation has a documented performance issue at `crates/search/src/query.rs:1011`:

```rust
// TODO: Filter conditions are inefficiently searched, especially in conjunction
// with text searches. We should eventually optimize this simpler implementation
// as well.
```

### Current Behavior

1. Execute text search → retrieve all matching documents
2. Load each document from storage
3. Apply filter conditions
4. Return filtered results

### Problems

1. **Redundant I/O**: Documents are loaded even if they'll be filtered out
2. **Memory pressure**: All candidates held in memory during filtering
3. **Latency**: Two sequential operations instead of one
4. **Poor scaling**: Gets worse as text matches increase

### Impact

This affects all queries that combine text search with filters—a common pattern in production applications (e.g., "search for 'invoice' where status = 'pending'").

## Detailed Design

### Architecture Change

```mermaid
graph TB
    subgraph "Current (Two-Phase)"
        A1[Text Search] --> B1[Load All Matches]
        B1 --> C1[Apply Filters]
        C1 --> D1[Return Results]
    end

    subgraph "Proposed (Integrated)"
        A2[Parse Query] --> B2[Build Combined Plan]
        B2 --> C2[Execute with Pushdown]
        C2 --> D2[Return Results]
    end
```

### Implementation Steps

#### Phase 1: Filter Pushdown for Equality

**File:** `crates/search/src/query.rs`

```rust
// Before
pub async fn execute_search_query(
    &self,
    search_query: TextSearchQuery,
    filters: Vec<FilterCondition>,
) -> anyhow::Result<Vec<Document>> {
    // Execute text search
    let candidates = self.text_index.search(&search_query).await?;

    // Load and filter
    let mut results = Vec::new();
    for doc_id in candidates {
        let doc = self.load_document(doc_id).await?;
        if self.matches_filters(&doc, &filters) {
            results.push(doc);
        }
    }
    Ok(results)
}

// After
pub async fn execute_search_query(
    &self,
    search_query: TextSearchQuery,
    filters: Vec<FilterCondition>,
) -> anyhow::Result<Vec<Document>> {
    // Build combined query
    let combined = CombinedQuery::new(search_query, filters);

    // Check if filters can be pushed down
    let (pushdown_filters, post_filters) = combined.partition_filters();

    // Execute with pushdown
    let candidates = self.text_index
        .search_with_filters(&combined.text, &pushdown_filters)
        .await?;

    // Apply remaining filters
    let mut results = Vec::new();
    for doc_id in candidates {
        let doc = self.load_document(doc_id).await?;
        if self.matches_filters(&doc, &post_filters) {
            results.push(doc);
        }
    }
    Ok(results)
}
```

#### Phase 2: Index Predicate Support

**File:** `crates/search/src/memory_index/mod.rs`

```rust
pub struct SearchWithFilters {
    text_query: TextQuery,
    filter_predicates: Vec<IndexPredicate>,
}

pub enum IndexPredicate {
    // Can be evaluated on index
    Equality { field: String, value: ConvexValue },
    Range { field: String, min: Option<ConvexValue>, max: Option<ConvexValue> },
    In { field: String, values: Vec<ConvexValue> },

    // Must be evaluated on document
    Complex(FilterCondition),
}

impl MemoryIndex {
    pub fn search_with_predicates(
        &self,
        query: &SearchWithFilters,
    ) -> Vec<DocumentId> {
        // Get text search results
        let text_matches = self.text_search(&query.text_query);

        // Apply index predicates during iteration
        text_matches
            .into_iter()
            .filter(|doc_id| {
                self.check_predicates(doc_id, &query.filter_predicates)
            })
            .collect()
    }
}
```

#### Phase 3: Combined Scoring

```rust
pub struct CombinedScore {
    text_relevance: f32,
    filter_boost: f32,
}

impl CombinedScore {
    pub fn compute(
        text_score: f32,
        filter_matches: &[bool],
        weights: &ScoreWeights,
    ) -> f32 {
        let filter_score = filter_matches.iter()
            .enumerate()
            .map(|(i, matched)| {
                if *matched { weights.filter_weights[i] } else { 0.0 }
            })
            .sum::<f32>();

        text_score * weights.text_weight + filter_score
    }
}
```

### Data Structures

```rust
pub struct CombinedQuery {
    pub text: TextSearchQuery,
    pub filters: Vec<FilterCondition>,
    pub pushdown_candidates: Vec<PushdownFilter>,
}

pub struct PushdownFilter {
    pub field: FieldPath,
    pub predicate: FilterPredicate,
    pub index_support: IndexSupport,
}

pub enum IndexSupport {
    FullySupported,      // Can evaluate entirely in index
    PartiallySupported,  // Can narrow candidates
    NotSupported,        // Must evaluate on document
}
```

## Example Usage

### Before

```typescript
// User query
const results = await ctx.db
  .query("documents")
  .withSearchIndex("search_content", q => q.search("query", "invoice"))
  .filter(q => q.eq(q.field("status"), "pending"))
  .collect();

// Internal: loads all "invoice" matches, then filters
```

### After

```typescript
// Same user query
const results = await ctx.db
  .query("documents")
  .withSearchIndex("search_content", q => q.search("query", "invoice"))
  .filter(q => q.eq(q.field("status"), "pending"))
  .collect();

// Internal: combines search and filter in index
```

No API changes required—optimization is transparent.

## Implementation Plan

### Milestones

| Phase | Duration | Deliverable |
|-------|----------|-------------|
| 1 | 3 days | Filter pushdown for equality predicates |
| 2 | 4 days | Index predicate support |
| 3 | 3 days | Combined scoring and benchmarks |

### Testing Strategy

1. **Unit tests**: Filter partitioning, predicate evaluation
2. **Integration tests**: Combined queries with various filter types
3. **Benchmarks**: Compare before/after performance
4. **Property tests**: Ensure results are identical

### Rollout

1. Feature flag for new implementation
2. Shadow mode: run both, compare results
3. Gradual rollout by tenant
4. Full deployment after validation

## Backwards Compatibility

This is a transparent optimization:
- **No API changes**
- **Same results** (validated in shadow mode)
- **Better performance**

Users don't need to change anything.

## Alternatives Considered

### 1. Separate Filter Index

Create a dedicated index for filter fields.

**Pros:** Fast filter evaluation
**Cons:** Storage overhead, sync complexity, doesn't help combined queries

### 2. Materialized Views

Pre-compute common search+filter combinations.

**Pros:** Extremely fast reads
**Cons:** Write overhead, storage cost, limited to known patterns

### 3. Query Plan Caching

Cache execution plans for repeated queries.

**Pros:** Helps repeated queries
**Cons:** Doesn't address fundamental inefficiency

The proposed approach is superior because it:
- Addresses root cause
- No storage overhead
- Works for all queries
- Transparent to users

## Open Questions

1. **Which filter types to support initially?**
   - Recommend: equality, range, in
   - Defer: regex, complex expressions

2. **How to handle conflicting scores?**
   - Recommend: configurable weights with sensible defaults

3. **Should we expose pushdown hints to users?**
   - Recommend: No, keep it transparent

## Success Criteria

- [ ] 50% reduction in p99 latency for search+filter queries
- [ ] No regression in search-only queries
- [ ] All existing tests pass
- [ ] Benchmarks show improvement across dataset sizes

## Effort Estimation

- **Engineering time:** 10 dev-days
- **Review:** 2 dev-days
- **Testing:** 3 dev-days
- **Rollout:** 3 days

**Total: ~2 weeks with 1 engineer**

---

*References:*
- [Query Implementation](https://github.com/get-convex/convex-backend/blob/482df8a0e1288c9410be8241deb06b09d7ff70b0/crates/search/src/query.rs)
- [Memory Index](https://github.com/get-convex/convex-backend/blob/482df8a0e1288c9410be8241deb06b09d7ff70b0/crates/search/src/memory_index/mod.rs)
