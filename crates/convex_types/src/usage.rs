//! Shared usage types to avoid circular dependencies between crates.
//!
//! This module contains types used by both `common` and `usage_tracking` crates
//! that would otherwise cause circular dependency issues.

use serde::Serialize;

/// User-facing UDF stats that are logged in the UDF execution log
/// and might be used for debugging purposes.
///
/// This type consolidates the previously duplicated `AggregatedFunctionUsageStats`
/// from `common::log_streaming` and `usage_tracking`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
#[cfg_attr(any(test, feature = "testing"), derive(proptest_derive::Arbitrary))]
pub struct AggregatedFunctionUsageStats {
    pub database_read_bytes: u64,
    pub database_write_bytes: u64,
    pub database_read_documents: u64,
    pub storage_read_bytes: u64,
    pub storage_write_bytes: u64,
    pub vector_index_read_bytes: u64,
    pub vector_index_write_bytes: u64,
    /// Memory used by the function in megabytes.
    /// This field is populated for queries, mutations, actions, and HTTP actions.
    pub memory_used_mb: u64,
    /// Size of the return value in bytes, if applicable.
    pub return_bytes: Option<u64>,
}

impl AggregatedFunctionUsageStats {
    /// Creates a new instance with basic usage metrics, without memory or return bytes info.
    /// This is useful when aggregating from `FunctionUsageStats`.
    pub fn from_basic_metrics(
        database_read_bytes: u64,
        database_write_bytes: u64,
        database_read_documents: u64,
        storage_read_bytes: u64,
        storage_write_bytes: u64,
        vector_index_read_bytes: u64,
        vector_index_write_bytes: u64,
    ) -> Self {
        Self {
            database_read_bytes,
            database_write_bytes,
            database_read_documents,
            storage_read_bytes,
            storage_write_bytes,
            vector_index_read_bytes,
            vector_index_write_bytes,
            memory_used_mb: 0,
            return_bytes: None,
        }
    }

    /// Sets the memory usage for this stats instance.
    pub fn with_memory(mut self, memory_used_mb: u64) -> Self {
        self.memory_used_mb = memory_used_mb;
        self
    }

    /// Sets the return bytes for this stats instance.
    pub fn with_return_bytes(mut self, return_bytes: Option<u64>) -> Self {
        self.return_bytes = return_bytes;
        self
    }
}

/// Information about Optimistic Concurrency Control (OCC) conflicts.
///
/// This type consolidates the previously duplicated `OccInfo` from
/// `common::log_streaming` and `usage_tracking`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(any(test, feature = "testing"), derive(proptest_derive::Arbitrary))]
pub struct OccInfo {
    pub table_name: Option<String>,
    pub document_id: Option<String>,
    pub write_source: Option<String>,
    pub retry_count: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_aggregated_function_usage_stats_default() {
        let stats = AggregatedFunctionUsageStats::default();
        assert_eq!(stats.database_read_bytes, 0);
        assert_eq!(stats.database_write_bytes, 0);
        assert_eq!(stats.database_read_documents, 0);
        assert_eq!(stats.storage_read_bytes, 0);
        assert_eq!(stats.storage_write_bytes, 0);
        assert_eq!(stats.vector_index_read_bytes, 0);
        assert_eq!(stats.vector_index_write_bytes, 0);
        assert_eq!(stats.memory_used_mb, 0);
        assert_eq!(stats.return_bytes, None);
    }

    #[test]
    fn test_from_basic_metrics() {
        let stats = AggregatedFunctionUsageStats::from_basic_metrics(100, 200, 10, 50, 60, 70, 80);
        assert_eq!(stats.database_read_bytes, 100);
        assert_eq!(stats.database_write_bytes, 200);
        assert_eq!(stats.database_read_documents, 10);
        assert_eq!(stats.storage_read_bytes, 50);
        assert_eq!(stats.storage_write_bytes, 60);
        assert_eq!(stats.vector_index_read_bytes, 70);
        assert_eq!(stats.vector_index_write_bytes, 80);
        assert_eq!(stats.memory_used_mb, 0);
        assert_eq!(stats.return_bytes, None);
    }

    #[test]
    fn test_builder_methods() {
        let stats = AggregatedFunctionUsageStats::from_basic_metrics(100, 200, 10, 50, 60, 70, 80)
            .with_memory(128)
            .with_return_bytes(Some(1024));

        assert_eq!(stats.memory_used_mb, 128);
        assert_eq!(stats.return_bytes, Some(1024));
    }

    #[test]
    fn test_occ_info() {
        let occ = OccInfo {
            table_name: Some("users".to_string()),
            document_id: Some("doc123".to_string()),
            write_source: Some("mutation".to_string()),
            retry_count: 3,
        };
        assert_eq!(occ.table_name, Some("users".to_string()));
        assert_eq!(occ.retry_count, 3);
    }
}
