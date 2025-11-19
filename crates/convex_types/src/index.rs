//! Shared index configuration types.
//!
//! This module is intended for types shared between the `search` and `vector`
//! crates to avoid duplication and potential drift between implementations.
//!
//! ## Future Work
//!
//! This module can be expanded to include:
//! - Common index configuration types
//! - Shared traits for index behavior
//! - Common segment handling types

use serde::Serialize;

/// Common index type enumeration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(any(test, feature = "testing"), derive(proptest_derive::Arbitrary))]
pub enum IndexType {
    /// Full-text search index
    FullText,
    /// Vector similarity search index
    Vector,
    /// Standard database index
    Database,
}

impl std::fmt::Display for IndexType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IndexType::FullText => write!(f, "full_text"),
            IndexType::Vector => write!(f, "vector"),
            IndexType::Database => write!(f, "database"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_index_type_display() {
        assert_eq!(IndexType::FullText.to_string(), "full_text");
        assert_eq!(IndexType::Vector.to_string(), "vector");
        assert_eq!(IndexType::Database.to_string(), "database");
    }
}
