//! Test isolation utilities to ensure tests don't interfere with each other.
//!
//! This module provides guards that help ensure test isolation by managing
//! environment variables and temporary directories.

use std::{
    collections::HashMap,
    env,
    path::PathBuf,
    sync::Mutex,
};

use tempfile::TempDir;

/// Guard that ensures test isolation by managing environment state and temp directories.
///
/// When created, `TestIsolation` will:
/// - Create an isolated temporary directory
/// - Set the `CONVEX_TEST_TEMP` environment variable to point to it
/// - Optionally save and restore environment variables on drop
///
/// # Example
///
/// ```ignore
/// #[test]
/// fn test_something() {
///     let isolation = TestIsolation::new();
///
///     // Test runs with isolated temp directory
///     let temp_path = isolation.temp_dir();
///
///     // All state is cleaned up when isolation drops
/// }
/// ```
pub struct TestIsolation {
    /// Temporary directory for this test
    temp_dir: TempDir,
    /// Saved environment variables to restore on drop
    saved_env: HashMap<String, Option<String>>,
    /// Keys to restore on drop
    keys_to_restore: Vec<String>,
}

impl TestIsolation {
    /// Create a new test isolation guard.
    ///
    /// This creates a temporary directory and sets up the test environment.
    pub fn new() -> Self {
        let temp_dir = TempDir::new().expect("Failed to create temp directory for test isolation");
        let mut saved_env = HashMap::new();
        let mut keys_to_restore = Vec::new();

        // Save and set CONVEX_TEST_TEMP
        let key = "CONVEX_TEST_TEMP";
        saved_env.insert(key.to_string(), env::var(key).ok());
        keys_to_restore.push(key.to_string());
        env::set_var(key, temp_dir.path());

        Self {
            temp_dir,
            saved_env,
            keys_to_restore,
        }
    }

    /// Create a new test isolation guard with additional environment variable overrides.
    ///
    /// # Arguments
    ///
    /// * `env_overrides` - Map of environment variable names to values to set for this test
    pub fn with_env(env_overrides: HashMap<String, String>) -> Self {
        let mut isolation = Self::new();

        for (key, value) in env_overrides {
            // Save the current value
            isolation
                .saved_env
                .insert(key.clone(), env::var(&key).ok());
            isolation.keys_to_restore.push(key.clone());
            // Set the override
            env::set_var(key, value);
        }

        isolation
    }

    /// Get the path to the isolated temporary directory.
    pub fn temp_dir(&self) -> &std::path::Path {
        self.temp_dir.path()
    }

    /// Get a PathBuf to the isolated temporary directory.
    pub fn temp_path(&self) -> PathBuf {
        self.temp_dir.path().to_path_buf()
    }

    /// Create a subdirectory within the temp directory.
    pub fn create_subdir(&self, name: &str) -> anyhow::Result<PathBuf> {
        let path = self.temp_dir.path().join(name);
        std::fs::create_dir_all(&path)?;
        Ok(path)
    }
}

impl Default for TestIsolation {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for TestIsolation {
    fn drop(&mut self) {
        // Restore environment variables
        for key in &self.keys_to_restore {
            match self.saved_env.get(key) {
                Some(Some(value)) => {
                    env::set_var(key, value);
                }
                Some(None) => {
                    env::remove_var(key);
                }
                None => {}
            }
        }
        // temp_dir will be automatically cleaned up when dropped
    }
}

/// A counter that tracks test execution for debugging isolation issues.
///
/// This is useful for verifying that tests run in isolation and don't share state.
pub struct TestCounter {
    counter: Mutex<usize>,
}

impl TestCounter {
    /// Create a new test counter starting at zero.
    pub const fn new() -> Self {
        Self {
            counter: Mutex::new(0),
        }
    }

    /// Increment the counter and return the new value.
    pub fn increment(&self) -> usize {
        let mut guard = self.counter.lock().unwrap();
        *guard += 1;
        *guard
    }

    /// Get the current counter value.
    pub fn get(&self) -> usize {
        *self.counter.lock().unwrap()
    }

    /// Reset the counter to zero.
    pub fn reset(&self) {
        *self.counter.lock().unwrap() = 0;
    }
}

impl Default for TestCounter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_isolation_creates_temp_dir() {
        let isolation = TestIsolation::new();
        assert!(isolation.temp_dir().exists());
        assert!(isolation.temp_dir().is_dir());
    }

    #[test]
    fn test_isolation_sets_env_var() {
        let isolation = TestIsolation::new();
        let temp_path = isolation.temp_dir().to_string_lossy().to_string();
        assert_eq!(env::var("CONVEX_TEST_TEMP").unwrap(), temp_path);
    }

    #[test]
    fn test_isolation_restores_env_on_drop() {
        // First set a known value
        env::set_var("CONVEX_TEST_TEMP", "original_value");

        {
            let _isolation = TestIsolation::new();
            // Env var should be changed
            assert_ne!(env::var("CONVEX_TEST_TEMP").unwrap(), "original_value");
        }

        // After drop, it should be restored
        assert_eq!(env::var("CONVEX_TEST_TEMP").unwrap(), "original_value");

        // Clean up
        env::remove_var("CONVEX_TEST_TEMP");
    }

    #[test]
    fn test_isolation_with_custom_env() {
        let mut overrides = HashMap::new();
        overrides.insert("MY_TEST_VAR".to_string(), "test_value".to_string());

        {
            let _isolation = TestIsolation::with_env(overrides);
            assert_eq!(env::var("MY_TEST_VAR").unwrap(), "test_value");
        }

        // Should be removed after drop
        assert!(env::var("MY_TEST_VAR").is_err());
    }

    #[test]
    fn test_counter() {
        let counter = TestCounter::new();
        assert_eq!(counter.get(), 0);
        assert_eq!(counter.increment(), 1);
        assert_eq!(counter.increment(), 2);
        counter.reset();
        assert_eq!(counter.get(), 0);
    }
}
