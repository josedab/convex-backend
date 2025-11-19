# RFC-0004: Test Reliability Program

**Status:** Draft
**Author:** Claude (AI Analysis)
**Created:** November 19, 2025
**Analysis Commit:** `482df8a0e1288c9410be8241deb06b09d7ff70b0`

---

## Summary

Systematically fix the 11 flaky/ignored tests and resolve test isolation issues to ensure CI reliability and enable confident refactoring across the codebase.

## Motivation

### Current State

The test suite has reliability issues that undermine developer confidence:

| Category | Count | Impact |
|----------|-------|--------|
| Ignored tests | 11 | Missing coverage |
| Flaky tests | 5+ | False negatives |
| Isolation issues | 2 | Test interference |

### Flaky/Ignored Test Inventory

**1. Isolate Tests (3)**

`crates/isolate/src/tests/adversarial.rs`:
```rust
#[test]
#[ignore] // Flaky on CI - timing dependent
fn test_timeout_in_nested_async() { ... }

#[test]
#[ignore] // Memory limit detection unreliable
fn test_memory_limit_exceeded() { ... }
```

`crates/isolate/src/tests/fetch.rs`:
```rust
#[test]
#[ignore] // TODO(ENG-7281) fix flakes
fn test_fetch_with_redirect() { ... }
```

**2. AWS S3 Tests (4)**

`crates/aws_s3/src/storage.rs`:
```rust
#[test]
#[ignore] // Requires AWS credentials
fn test_s3_upload() { ... }

#[test]
#[ignore] // Requires AWS credentials
fn test_s3_signed_url() { ... }

#[test]
#[ignore] // Network dependent
fn test_s3_multipart_upload() { ... }

#[test]
#[ignore] // Requires specific S3 bucket
fn test_s3_presigned_upload() { ... }
```

**3. Database Tests (2)**

`crates/database/src/tests/vector_tests.rs`:
```rust
// This test is flaky - vector similarity scores vary slightly
#[test]
fn test_vector_search_relevance() { ... }
```

`crates/application/src/snapshot_import/tests.rs`:
```rust
#[test]
#[ignore] // TODO(CX-5143): Re-enable after fixing the flake
fn test_import_large_snapshot() { ... }
```

**4. Search Tests (1)**

`crates/search/src/searcher/searcher.rs`:
```rust
#[test]
#[ignore] // Tantivy scoring is non-deterministic
fn test_fuzzy_search_ordering() { ... }
```

### Test Isolation Issues

`crates/application/src/tests/scheduled_jobs.rs:193`:
```rust
fn test_scheduled_job_execution() {
    // TODO: this is sketchy and could interfere with other tests in this process
    static GLOBAL_JOB_COUNTER: AtomicUsize = AtomicUsize::new(0);

    // This modifies global state that other tests might read
    GLOBAL_JOB_COUNTER.fetch_add(1, Ordering::SeqCst);
}
```

### Impact

1. **False Confidence**: Green CI doesn't guarantee working code
2. **Developer Friction**: "Just re-run it" culture
3. **Hidden Bugs**: Real failures masked by known flakes
4. **Refactoring Fear**: Developers avoid touching areas with flaky tests
5. **CI Costs**: Wasted compute on retries

## Detailed Design

### Phase 1: Test Triage and Classification (1 day)

Categorize each flaky test:

| Category | Description | Fix Strategy |
|----------|-------------|--------------|
| **Timing** | Depends on wall-clock time | Use TestRuntime |
| **Resource** | Depends on system resources | Mock or skip in CI |
| **Network** | Depends on external services | Mock or integration-only |
| **State** | Global state interference | Isolation guards |
| **Non-determinism** | Random behavior | Seed or tolerance |

### Phase 2: Test Isolation Framework (1 day)

Create a framework for test isolation:

```rust
// crates/testing/src/isolation.rs

/// Guard that ensures test isolation
pub struct TestIsolation {
    // Saved state to restore
    saved_env: HashMap<String, String>,
    saved_config: Option<Config>,
    temp_dir: Option<TempDir>,
}

impl TestIsolation {
    pub fn new() -> Self {
        let mut isolation = Self {
            saved_env: std::env::vars().collect(),
            saved_config: Config::current().cloned(),
            temp_dir: None,
        };

        // Create isolated temp directory
        isolation.temp_dir = Some(TempDir::new().unwrap());

        // Set isolated environment
        std::env::set_var("CONVEX_TEST_TEMP", isolation.temp_dir().path());

        isolation
    }

    pub fn temp_dir(&self) -> &TempDir {
        self.temp_dir.as_ref().unwrap()
    }
}

impl Drop for TestIsolation {
    fn drop(&mut self) {
        // Restore environment
        for (key, value) in &self.saved_env {
            std::env::set_var(key, value);
        }

        // Restore config
        if let Some(config) = &self.saved_config {
            Config::set_current(config.clone());
        }
    }
}
```

**Usage:**

```rust
#[test]
fn test_scheduled_job_execution() {
    let _isolation = TestIsolation::new();

    // Test runs in isolated environment
    // All state is cleaned up when _isolation drops
}
```

### Phase 3: Fix Timing-Dependent Tests (1 day)

Convert tests to use TestRuntime:

**Before:**
```rust
#[tokio::test]
async fn test_timeout_in_nested_async() {
    let result = with_timeout(Duration::from_millis(100)).await;
    assert!(result.is_err()); // FLAKY: might succeed on fast systems
}
```

**After:**
```rust
#[convex_macro::test_runtime]
async fn test_timeout_in_nested_async(rt: TestRuntime) {
    // TestRuntime virtualizes time
    let result = with_timeout(&rt, Duration::from_millis(100)).await;
    assert!(result.is_err()); // DETERMINISTIC
}
```

### Phase 4: Fix AWS S3 Tests (0.5 day)

Make S3 tests work with mocks for CI:

```rust
// crates/aws_s3/src/tests/mock_s3.rs

pub struct MockS3 {
    objects: Arc<Mutex<HashMap<String, Vec<u8>>>>,
}

impl MockS3 {
    pub fn new() -> Self {
        Self {
            objects: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

#[async_trait]
impl Storage for MockS3 {
    async fn start_upload(&self) -> anyhow::Result<Box<BufferedUpload>> {
        Ok(Box::new(MockUpload::new(self.objects.clone())))
    }

    async fn signed_url(&self, key: ObjectKey, _expires: Duration)
        -> anyhow::Result<String> {
        // Return mock URL
        Ok(format!("mock://s3/{}", key))
    }
}
```

**Updated Tests:**

```rust
#[tokio::test]
async fn test_s3_upload() {
    let storage: Box<dyn Storage> = if cfg!(feature = "integration_tests") {
        Box::new(S3Storage::from_env().await.unwrap())
    } else {
        Box::new(MockS3::new())
    };

    let mut upload = storage.start_upload().await.unwrap();
    upload.write(b"test data").await.unwrap();
    let key = upload.finish().await.unwrap();

    assert!(key.to_string().len() > 0);
}
```

### Phase 5: Fix Non-Deterministic Tests (1 day)

**Vector Search:**

```rust
#[test]
fn test_vector_search_relevance() {
    // Use tolerance for floating-point comparison
    let results = vector_search(&query, limit: 10).await;

    // Check ordering, not exact scores
    assert!(results[0].score >= results[1].score);
    assert!(results[1].score >= results[2].score);

    // Use tolerance for score comparison
    assert!((results[0].score - expected_score).abs() < 0.01);
}
```

**Fuzzy Search:**

```rust
#[test]
fn test_fuzzy_search_ordering() {
    // Seed the search for determinism
    let searcher = Searcher::new_with_seed(42);

    let results = searcher.fuzzy_search("test").await;

    // Deterministic results with seeded searcher
    assert_eq!(results[0].id, expected_first_id);
}
```

### Phase 6: Fix Import Tests (0.5 day)

The large snapshot import test likely has resource contention:

```rust
#[tokio::test]
async fn test_import_large_snapshot() {
    // Use isolated database
    let _isolation = TestIsolation::new();
    let db = TestDatabase::new(&_isolation.temp_dir()).await;

    // Generate test data
    let snapshot = generate_test_snapshot(10_000); // rows

    // Import with proper timeout
    let result = tokio::time::timeout(
        Duration::from_secs(60),
        db.import_snapshot(snapshot)
    ).await;

    assert!(result.is_ok(), "Import timed out");
    assert!(result.unwrap().is_ok(), "Import failed");
}
```

## Example Usage

### Before: Test with Global State

```rust
static COUNTER: AtomicUsize = AtomicUsize::new(0);

#[test]
fn test_a() {
    COUNTER.fetch_add(1, Ordering::SeqCst);
    assert_eq!(COUNTER.load(Ordering::SeqCst), 1);
}

#[test]
fn test_b() {
    COUNTER.fetch_add(1, Ordering::SeqCst);
    assert_eq!(COUNTER.load(Ordering::SeqCst), 1); // FAILS if test_a runs first!
}
```

### After: Isolated Tests

```rust
#[test]
fn test_a() {
    let _isolation = TestIsolation::new();
    let counter = Counter::new();
    counter.increment();
    assert_eq!(counter.get(), 1);
}

#[test]
fn test_b() {
    let _isolation = TestIsolation::new();
    let counter = Counter::new();
    counter.increment();
    assert_eq!(counter.get(), 1); // ALWAYS passes
}
```

## Implementation Plan

### Milestones

| Phase | Duration | Tests Fixed | Risk |
|-------|----------|-------------|------|
| 1. Triage | 1 day | 0 (categorization) | None |
| 2. Isolation framework | 1 day | 2 (scheduled_jobs) | Low |
| 3. Timing fixes | 1 day | 3 (isolate tests) | Low |
| 4. AWS mocks | 0.5 day | 4 (S3 tests) | Medium |
| 5. Non-determinism | 1 day | 2 (vector, search) | Low |
| 6. Import test | 0.5 day | 1 (snapshot_import) | Low |

### Testing Strategy

1. **Before**: Run full test suite 10 times, note failures
2. **After each phase**: Run affected tests 10 times
3. **Final**: Run full suite 20 times with no failures

### CI Integration

Update nextest configuration:

```toml
# .config/nextest.toml

[profile.ci]
# No more retries - tests must be reliable
retries = 0

# Fail fast on first failure
fail-fast = true

# Enable test isolation
test-threads = "num-cpus"
```

## Backwards Compatibility

- **No API changes**: Internal test infrastructure only
- **Better reliability**: Tests become trustworthy
- **Same coverage**: All tests remain, none removed

## Alternatives Considered

### 1. Delete Flaky Tests

Simply remove problematic tests.

**Pros:** Immediate green CI
**Cons:**
- Lost coverage
- Bugs will slip through
- Doesn't solve underlying issues

### 2. Retry in CI

Automatically retry failed tests.

**Pros:** Quick fix
**Cons:**
- Masks real failures
- Increases CI time
- Doesn't fix the problem

### 3. Quarantine Tests

Mark flaky tests to not fail CI.

**Pros:** Tracks flaky tests
**Cons:**
- Still no coverage
- Tests may never get fixed

The proposed approach fixes tests properly, ensuring real coverage.

## Open Questions

1. **Should AWS tests require credentials in CI?**
   - Recommendation: Use mocks for CI, credentials for integration

2. **How to handle inherently non-deterministic tests?**
   - Recommendation: Use tolerances or property-based testing

3. **Should we add flakiness tracking?**
   - Recommendation: Yes, track historical flakiness per test

## Success Criteria

- [ ] 0 ignored tests (all enabled or converted to integration tests)
- [ ] 0 test isolation issues documented
- [ ] CI passes 100% for 1 week straight
- [ ] All tests deterministic (same result on every run)
- [ ] Test suite runs without retries

## Effort Estimation

| Task | Effort |
|------|--------|
| Phase 1: Triage | 0.5 dev-days |
| Phase 2: Isolation framework | 1 dev-day |
| Phase 3: Timing fixes | 0.5 dev-days |
| Phase 4: AWS mocks | 0.5 dev-days |
| Phase 5: Non-determinism | 0.5 dev-days |
| Phase 6: Import test | 0.25 dev-days |
| Validation | 0.25 dev-days |

**Total: 3-4 dev-days**

## Stakeholders

- **Engineering**: Required - implements fixes
- **DevOps**: Interested - CI reliability
- **All developers**: Benefit - confident refactoring

---

*References:*
- [Nextest Config](https://github.com/get-convex/convex-backend/blob/482df8a0e1288c9410be8241deb06b09d7ff70b0/.config/nextest.toml)
- [Isolate Tests](https://github.com/get-convex/convex-backend/blob/482df8a0e1288c9410be8241deb06b09d7ff70b0/crates/isolate/src/tests/)
- [Scheduled Jobs Tests](https://github.com/get-convex/convex-backend/blob/482df8a0e1288c9410be8241deb06b09d7ff70b0/crates/application/src/tests/scheduled_jobs.rs)
