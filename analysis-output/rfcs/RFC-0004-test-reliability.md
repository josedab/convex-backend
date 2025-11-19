# RFC-0004: Test Reliability Program

**Status:** Draft
**Author:** Claude (AI Analysis)
**Created:** November 19, 2025
**Analysis Commit:** `482df8a0e1288c9410be8241deb06b09d7ff70b0`

---

## Summary

Fix the 11 flaky/ignored tests and resolve test isolation issues to ensure confident CI/CD and enable safe refactoring.

## Motivation

### Flaky/Ignored Tests

| Location | Issue |
|----------|-------|
| `isolate/src/tests/adversarial.rs` | 2 ignored tests |
| `isolate/src/tests/fetch.rs` | 1 ignored (flake) |
| `aws_s3/src/storage.rs` | 4 ignored tests |
| `database/src/tests/vector_tests.rs` | Documented flake |
| `application/src/snapshot_import/tests.rs` | 1 ignored |
| `search/src/searcher/searcher.rs` | 1 ignored |

Example:
```rust
#[ignore] // TODO(CX-5143): Re-enable this test after fixing the flake.
```

### Test Isolation Issues

**Location:** `crates/application/src/tests/scheduled_jobs.rs:193`
```rust
// TODO: this is sketchy and could interfere with other tests in this process
```

### Impact

- **False confidence**: Green CI doesn't mean code works
- **Ignored failures**: Real bugs could be masked
- **Fear of change**: Developers avoid touching flaky areas
- **CI flakiness**: Random failures slow development

## Detailed Design

### Phase 1: Triage Ignored Tests (1 day)

For each ignored test:
1. Document the failure mode
2. Categorize: environment, timing, resource, logic
3. Assess fix difficulty
4. Prioritize by impact

### Phase 2: Fix Test Isolation (1 day)

**Problem:**
```rust
// Modifies global state
fn test_scheduled_job() {
    set_global_config(test_config);  // Affects other tests!
}
```

**Solution:**
```rust
fn test_scheduled_job() {
    let _guard = TestIsolation::new();
    // All changes reverted when guard drops
}

struct TestIsolation {
    original_config: Config,
}

impl Drop for TestIsolation {
    fn drop(&mut self) {
        restore_config(self.original_config);
    }
}
```

### Phase 3: Fix Timing Issues (1 day)

**Problem:**
```rust
async fn test_fetch_timeout() {
    let result = fetch_with_timeout(Duration::from_millis(100)).await;
    assert!(result.is_err()); // Flaky: might succeed on fast systems
}
```

**Solution:**
```rust
#[convex_macro::test_runtime]
async fn test_fetch_timeout(rt: TestRuntime) {
    // Use virtualized time
    let result = fetch_with_timeout(&rt, Duration::from_millis(100)).await;
    assert!(result.is_err()); // Deterministic
}
```

## Implementation Plan

| Phase | Duration | Tests Fixed |
|-------|----------|-------------|
| 1. Triage | 1 day | 0 (categorization) |
| 2. Isolation | 1 day | 2 |
| 3. Timing | 1 day | 5 |
| 4. Environment | 0.5 day | 4 (AWS tests) |

## Success Criteria

- [ ] 0 ignored tests (all enabled or explicitly skipped with reason)
- [ ] 0 test isolation issues
- [ ] CI passes 100% for 1 week
- [ ] All tests deterministic

## Effort Estimation

**Total: 3-4 dev-days**

---

*References:*
- [Nextest Config](https://github.com/get-convex/convex-backend/blob/482df8a0e1288c9410be8241deb06b09d7ff70b0/.config/nextest.toml)
