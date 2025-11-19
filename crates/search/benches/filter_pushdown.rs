//! Benchmarks for filter pushdown optimization.
//!
//! These benchmarks measure the performance improvement from pushing filter
//! predicates into the search index evaluation.

#![feature(never_type)]
#![feature(try_blocks)]

use search::{
    query::CompiledQuery,
    CombinedQuery,
    CombinedScore,
    PushdownConfig,
    PushdownStats,
    QueryExecutionPlan,
    ScoreWeights,
};

// Comment this out if you don't need memory profiling.
#[global_allocator]
static ALLOC: divan::AllocProfiler = divan::AllocProfiler::system();

#[divan::bench(max_time = 2)]
fn create_combined_query_empty(bencher: divan::Bencher) {
    let query = CompiledQuery {
        text_query: vec![],
        filter_conditions: vec![],
    };
    bencher.bench(|| CombinedQuery::new(query.clone()));
}

#[divan::bench(max_time = 2)]
fn partition_filters_empty(bencher: divan::Bencher) {
    let query = CompiledQuery {
        text_query: vec![],
        filter_conditions: vec![],
    };
    let combined = CombinedQuery::new(query);
    bencher.bench(|| combined.partition_filters());
}

#[divan::bench(max_time = 2)]
fn compute_combined_score_no_filters(bencher: divan::Bencher) {
    let weights = ScoreWeights::default();
    bencher.bench(|| CombinedScore::compute(2.5, &[], &weights));
}

fn filter_count_args() -> impl Iterator<Item = usize> {
    [1, 2, 5, 10].into_iter()
}

#[divan::bench(
    args = filter_count_args(),
    max_time = 2,
)]
fn compute_combined_score_with_filters(bencher: divan::Bencher, num_filters: usize) {
    let weights = ScoreWeights::with_uniform_filters(1.0, num_filters, 0.5);
    let filter_matches: Vec<bool> = (0..num_filters).map(|i| i % 2 == 0).collect();
    bencher.bench(|| CombinedScore::compute(2.5, &filter_matches, &weights));
}

#[divan::bench(max_time = 2)]
fn create_execution_plan_disabled(bencher: divan::Bencher) {
    let query = CompiledQuery {
        text_query: vec![],
        filter_conditions: vec![],
    };
    let combined = CombinedQuery::new(query);
    let config = PushdownConfig {
        enabled: false,
        ..Default::default()
    };
    bencher.bench(|| QueryExecutionPlan::from_combined_query(&combined, &config));
}

#[divan::bench(max_time = 2)]
fn create_execution_plan_enabled(bencher: divan::Bencher) {
    let query = CompiledQuery {
        text_query: vec![],
        filter_conditions: vec![],
    };
    let combined = CombinedQuery::new(query);
    let config = PushdownConfig::default();
    bencher.bench(|| QueryExecutionPlan::from_combined_query(&combined, &config));
}

#[divan::bench(max_time = 2)]
fn pushdown_stats_effectiveness(bencher: divan::Bencher) {
    let stats = PushdownStats {
        total_filters: 100,
        pushed_down: 75,
        post_evaluated: 25,
        docs_eliminated_by_pushdown: 1000,
    };
    bencher.bench(|| stats.effectiveness());
}

fn main() {
    divan::main();
}
