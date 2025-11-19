//! Shared types with minimal dependencies.
//!
//! This crate contains types used across multiple crates to avoid circular
//! dependencies. Types in this crate should have minimal dependencies to
//! ensure they can be imported by any crate in the workspace.
//!
//! ## Usage
//!
//! Instead of duplicating types across crates to avoid circular dependencies,
//! shared types should be defined here and imported by both crates.
//!
//! ## Modules
//!
//! - [`usage`] - Usage tracking types shared between `common` and `usage_tracking`
//! - [`index`] - Index configuration types shared between `search` and `vector`
//! - [`streaming`] - Streaming-related types

pub mod index;
pub mod streaming;
pub mod usage;

// Re-export commonly used types at the crate root for convenience
pub use usage::{
    AggregatedFunctionUsageStats,
    OccInfo,
};
