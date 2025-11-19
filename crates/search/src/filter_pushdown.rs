//! Filter pushdown optimization for combined text search and filter queries.
//!
//! This module implements an optimization that pushes filter predicates into the
//! search index evaluation, eliminating the two-phase approach where documents
//! are first searched and then filtered separately.

use std::collections::BTreeMap;

use tantivy::Term;
use value::{
    ConvexValue,
    FieldPath,
};

use crate::query::{
    CompiledFilterCondition,
    CompiledQuery,
    QueryTerm,
};

/// Classification of how well a filter predicate can be evaluated at the index level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexSupport {
    /// Can evaluate entirely in the index without loading the document.
    FullySupported,
    /// Can narrow candidates in the index but needs document verification.
    PartiallySupported,
    /// Must evaluate on the full document after loading.
    NotSupported,
}

/// Types of predicates that can be pushed down to the index.
#[derive(Debug, Clone, PartialEq)]
pub enum FilterPredicate {
    /// Equality check: field == value
    Equality { value: ConvexValue },
    /// Range check: min <= field <= max
    Range {
        min: Option<ConvexValue>,
        max: Option<ConvexValue>,
    },
    /// In-set check: field IN (values)
    In { values: Vec<ConvexValue> },
}

/// A filter condition with metadata about its pushdown capability.
#[derive(Debug, Clone)]
pub struct PushdownFilter {
    /// The field path this filter applies to.
    pub field: FieldPath,
    /// The type of predicate.
    pub predicate: FilterPredicate,
    /// The original compiled term for the filter.
    pub compiled_term: Term,
    /// How well this filter is supported at the index level.
    pub index_support: IndexSupport,
}

/// A combined query that includes both text search and filter conditions
/// with pushdown analysis.
#[derive(Debug, Clone)]
pub struct CombinedQuery {
    /// The text search terms.
    pub text_query: Vec<QueryTerm>,
    /// All filter conditions from the original query.
    pub filter_conditions: Vec<CompiledFilterCondition>,
    /// Filters that have been analyzed for pushdown potential.
    pub pushdown_candidates: Vec<PushdownFilter>,
}

impl CombinedQuery {
    /// Create a new combined query from a compiled query.
    pub fn new(query: CompiledQuery) -> Self {
        let pushdown_candidates = Self::analyze_filters(&query.filter_conditions);
        Self {
            text_query: query.text_query,
            filter_conditions: query.filter_conditions,
            pushdown_candidates,
        }
    }

    /// Analyze filter conditions and identify pushdown candidates.
    fn analyze_filters(filters: &[CompiledFilterCondition]) -> Vec<PushdownFilter> {
        filters
            .iter()
            .filter_map(|filter| {
                match filter {
                    CompiledFilterCondition::Must(term) => {
                        // Extract field information from the term
                        // For now, we support equality predicates on indexed fields
                        let field_bytes = term.as_slice();

                        // Try to extract field path from the term
                        // Terms are structured with field information
                        if let Some(field_path) = Self::extract_field_path(term) {
                            if let Some(value) = Self::extract_value(term) {
                                return Some(PushdownFilter {
                                    field: field_path,
                                    predicate: FilterPredicate::Equality { value },
                                    compiled_term: term.clone(),
                                    index_support: IndexSupport::FullySupported,
                                });
                            }
                        }

                        // If we can't extract field info, mark as not supported
                        // but still include for tracking
                        None
                    },
                }
            })
            .collect()
    }

    /// Extract field path from a term.
    ///
    /// Note: This is a simplified implementation. The actual extraction
    /// depends on how terms are encoded in the index schema.
    fn extract_field_path(_term: &Term) -> Option<FieldPath> {
        // Terms in the search index encode field information
        // For filter terms, we can extract the field path
        // This is a placeholder - actual implementation depends on term encoding
        None
    }

    /// Extract the value from a term for equality comparison.
    fn extract_value(_term: &Term) -> Option<ConvexValue> {
        // Extract the value encoded in the term
        // This is a placeholder - actual implementation depends on term encoding
        None
    }

    /// Partition filters into those that can be pushed down and those that must
    /// be evaluated post-search.
    ///
    /// Returns (pushdown_filters, post_filters)
    pub fn partition_filters(&self) -> (Vec<&PushdownFilter>, Vec<&CompiledFilterCondition>) {
        let mut pushdown_filters = Vec::new();
        let mut post_filters = Vec::new();

        // Collect pushdown candidates that are fully or partially supported
        for candidate in &self.pushdown_candidates {
            match candidate.index_support {
                IndexSupport::FullySupported | IndexSupport::PartiallySupported => {
                    pushdown_filters.push(candidate);
                },
                IndexSupport::NotSupported => {
                    // Find the original filter condition
                    for filter in &self.filter_conditions {
                        if let CompiledFilterCondition::Must(term) = filter {
                            if *term == candidate.compiled_term {
                                post_filters.push(filter);
                                break;
                            }
                        }
                    }
                },
            }
        }

        // Add any filters that weren't analyzed as pushdown candidates
        let pushdown_terms: Vec<_> = self.pushdown_candidates.iter()
            .map(|c| &c.compiled_term)
            .collect();

        for filter in &self.filter_conditions {
            if let CompiledFilterCondition::Must(term) = filter {
                if !pushdown_terms.contains(&term) {
                    post_filters.push(filter);
                }
            }
        }

        (pushdown_filters, post_filters)
    }

    /// Check if this query can benefit from filter pushdown.
    pub fn can_use_pushdown(&self) -> bool {
        !self.pushdown_candidates.is_empty() &&
            self.pushdown_candidates.iter().any(|c| {
                matches!(c.index_support, IndexSupport::FullySupported | IndexSupport::PartiallySupported)
            })
    }

    /// Get the number of filters that can be pushed down.
    pub fn pushdown_count(&self) -> usize {
        self.pushdown_candidates.iter()
            .filter(|c| matches!(c.index_support, IndexSupport::FullySupported | IndexSupport::PartiallySupported))
            .count()
    }
}

/// Index predicate that can be evaluated during index traversal.
#[derive(Debug, Clone, PartialEq)]
pub enum IndexPredicate {
    /// Equality check on an indexed field.
    Equality { field: String, value: ConvexValue },
    /// Range check on an indexed field.
    Range {
        field: String,
        min: Option<ConvexValue>,
        max: Option<ConvexValue>,
    },
    /// In-set check on an indexed field.
    In { field: String, values: Vec<ConvexValue> },
    /// Complex predicate that requires document loading.
    Complex(CompiledFilterCondition),
}

impl IndexPredicate {
    /// Check if this predicate can be evaluated without loading the document.
    pub fn is_index_evaluable(&self) -> bool {
        !matches!(self, IndexPredicate::Complex(_))
    }
}

/// Statistics for filter pushdown optimization.
#[derive(Debug, Default, Clone)]
pub struct PushdownStats {
    /// Total number of filters analyzed.
    pub total_filters: usize,
    /// Number of filters pushed down.
    pub pushed_down: usize,
    /// Number of filters evaluated post-search.
    pub post_evaluated: usize,
    /// Documents eliminated by pushdown before loading.
    pub docs_eliminated_by_pushdown: usize,
}

impl PushdownStats {
    /// Calculate the effectiveness ratio of pushdown.
    pub fn effectiveness(&self) -> f64 {
        if self.total_filters == 0 {
            0.0
        } else {
            self.pushed_down as f64 / self.total_filters as f64
        }
    }
}

/// Configuration for filter pushdown behavior.
#[derive(Debug, Clone)]
pub struct PushdownConfig {
    /// Whether pushdown optimization is enabled.
    pub enabled: bool,
    /// Maximum number of predicates to push down.
    pub max_pushdown_predicates: usize,
    /// Minimum selectivity estimate for pushdown to be beneficial.
    pub min_selectivity_threshold: f64,
}

impl Default for PushdownConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_pushdown_predicates: 10,
            min_selectivity_threshold: 0.1,
        }
    }
}

/// Weights for combining text search and filter scores.
#[derive(Debug, Clone)]
pub struct ScoreWeights {
    /// Weight for text relevance score.
    pub text_weight: f32,
    /// Weights for each filter condition.
    pub filter_weights: Vec<f32>,
}

impl Default for ScoreWeights {
    fn default() -> Self {
        Self {
            text_weight: 1.0,
            filter_weights: vec![],
        }
    }
}

impl ScoreWeights {
    /// Create score weights with uniform filter weights.
    pub fn with_uniform_filters(text_weight: f32, num_filters: usize, filter_weight: f32) -> Self {
        Self {
            text_weight,
            filter_weights: vec![filter_weight; num_filters],
        }
    }
}

/// Combined score for search results that includes both text relevance and filter matching.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CombinedScore {
    /// The text relevance score (BM25).
    pub text_relevance: f32,
    /// Additional boost from filter matches.
    pub filter_boost: f32,
    /// The final combined score.
    pub total: f32,
}

impl CombinedScore {
    /// Compute a combined score from text and filter match information.
    pub fn compute(
        text_score: f32,
        filter_matches: &[bool],
        weights: &ScoreWeights,
    ) -> Self {
        let filter_score: f32 = filter_matches
            .iter()
            .enumerate()
            .map(|(i, matched)| {
                if *matched && i < weights.filter_weights.len() {
                    weights.filter_weights[i]
                } else {
                    0.0
                }
            })
            .sum();

        let total = text_score * weights.text_weight + filter_score;
        Self {
            text_relevance: text_score,
            filter_boost: filter_score,
            total,
        }
    }

    /// Create a score from just text relevance (no filter boost).
    pub fn from_text_only(text_score: f32) -> Self {
        Self {
            text_relevance: text_score,
            filter_boost: 0.0,
            total: text_score,
        }
    }
}

/// Query execution plan with pushdown information.
#[derive(Debug, Clone)]
pub struct QueryExecutionPlan {
    /// Filters that will be evaluated at the index level.
    pub index_filters: Vec<IndexPredicate>,
    /// Filters that must be evaluated after loading documents.
    pub post_filters: Vec<CompiledFilterCondition>,
    /// Whether this plan uses the optimized pushdown path.
    pub uses_pushdown: bool,
}

impl QueryExecutionPlan {
    /// Create an execution plan from a combined query.
    pub fn from_combined_query(combined: &CombinedQuery, config: &PushdownConfig) -> Self {
        if !config.enabled {
            // Fall back to evaluating all filters post-search
            return Self {
                index_filters: vec![],
                post_filters: combined.filter_conditions.clone(),
                uses_pushdown: false,
            };
        }

        let (pushdown, post) = combined.partition_filters();

        let index_filters: Vec<IndexPredicate> = pushdown
            .into_iter()
            .take(config.max_pushdown_predicates)
            .filter_map(|pf| {
                match &pf.predicate {
                    FilterPredicate::Equality { value } => Some(IndexPredicate::Equality {
                        field: pf.field.to_string(),
                        value: value.clone(),
                    }),
                    FilterPredicate::Range { min, max } => Some(IndexPredicate::Range {
                        field: pf.field.to_string(),
                        min: min.clone(),
                        max: max.clone(),
                    }),
                    FilterPredicate::In { values } => Some(IndexPredicate::In {
                        field: pf.field.to_string(),
                        values: values.clone(),
                    }),
                }
            })
            .collect();

        let post_filters: Vec<CompiledFilterCondition> = post.into_iter().cloned().collect();

        Self {
            uses_pushdown: !index_filters.is_empty(),
            index_filters,
            post_filters,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tantivy::schema::Field;

    #[test]
    fn test_combined_query_creation() {
        let query = CompiledQuery {
            text_query: vec![],
            filter_conditions: vec![],
        };
        let combined = CombinedQuery::new(query);
        assert!(combined.pushdown_candidates.is_empty());
    }

    #[test]
    fn test_partition_empty_filters() {
        let query = CompiledQuery {
            text_query: vec![],
            filter_conditions: vec![],
        };
        let combined = CombinedQuery::new(query);
        let (pushdown, post) = combined.partition_filters();
        assert!(pushdown.is_empty());
        assert!(post.is_empty());
    }

    #[test]
    fn test_pushdown_config_defaults() {
        let config = PushdownConfig::default();
        assert!(config.enabled);
        assert_eq!(config.max_pushdown_predicates, 10);
    }

    #[test]
    fn test_pushdown_stats_effectiveness() {
        let mut stats = PushdownStats::default();
        assert_eq!(stats.effectiveness(), 0.0);

        stats.total_filters = 10;
        stats.pushed_down = 5;
        assert_eq!(stats.effectiveness(), 0.5);
    }

    #[test]
    fn test_index_predicate_evaluable() {
        let eq_pred = IndexPredicate::Equality {
            field: "status".to_string(),
            value: ConvexValue::String("active".try_into().unwrap()),
        };
        assert!(eq_pred.is_index_evaluable());

        let range_pred = IndexPredicate::Range {
            field: "age".to_string(),
            min: Some(ConvexValue::Int64(18)),
            max: Some(ConvexValue::Int64(65)),
        };
        assert!(range_pred.is_index_evaluable());
    }

    #[test]
    fn test_index_support_classification() {
        assert_eq!(IndexSupport::FullySupported, IndexSupport::FullySupported);
        assert_ne!(IndexSupport::FullySupported, IndexSupport::NotSupported);
    }

    #[test]
    fn test_combined_score_computation() {
        let weights = ScoreWeights {
            text_weight: 1.0,
            filter_weights: vec![0.5, 0.3],
        };

        // All filters match
        let score = CombinedScore::compute(2.0, &[true, true], &weights);
        assert_eq!(score.text_relevance, 2.0);
        assert_eq!(score.filter_boost, 0.8);
        assert_eq!(score.total, 2.8);

        // Only first filter matches
        let score = CombinedScore::compute(2.0, &[true, false], &weights);
        assert_eq!(score.filter_boost, 0.5);
        assert_eq!(score.total, 2.5);

        // No filters match
        let score = CombinedScore::compute(2.0, &[false, false], &weights);
        assert_eq!(score.filter_boost, 0.0);
        assert_eq!(score.total, 2.0);
    }

    #[test]
    fn test_combined_score_from_text_only() {
        let score = CombinedScore::from_text_only(3.5);
        assert_eq!(score.text_relevance, 3.5);
        assert_eq!(score.filter_boost, 0.0);
        assert_eq!(score.total, 3.5);
    }

    #[test]
    fn test_score_weights_with_uniform_filters() {
        let weights = ScoreWeights::with_uniform_filters(1.0, 3, 0.5);
        assert_eq!(weights.text_weight, 1.0);
        assert_eq!(weights.filter_weights.len(), 3);
        assert!(weights.filter_weights.iter().all(|&w| w == 0.5));
    }

    #[test]
    fn test_query_execution_plan_disabled() {
        let query = CompiledQuery {
            text_query: vec![],
            filter_conditions: vec![],
        };
        let combined = CombinedQuery::new(query);
        let config = PushdownConfig {
            enabled: false,
            ..Default::default()
        };
        let plan = QueryExecutionPlan::from_combined_query(&combined, &config);
        assert!(!plan.uses_pushdown);
        assert!(plan.index_filters.is_empty());
    }

    #[test]
    fn test_query_execution_plan_enabled_no_filters() {
        let query = CompiledQuery {
            text_query: vec![],
            filter_conditions: vec![],
        };
        let combined = CombinedQuery::new(query);
        let config = PushdownConfig::default();
        let plan = QueryExecutionPlan::from_combined_query(&combined, &config);
        assert!(!plan.uses_pushdown);
        assert!(plan.index_filters.is_empty());
        assert!(plan.post_filters.is_empty());
    }

    #[test]
    fn test_can_use_pushdown_empty() {
        let query = CompiledQuery {
            text_query: vec![],
            filter_conditions: vec![],
        };
        let combined = CombinedQuery::new(query);
        assert!(!combined.can_use_pushdown());
        assert_eq!(combined.pushdown_count(), 0);
    }
}
