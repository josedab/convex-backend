#![feature(iterator_try_collect)]

use std::{
    collections::BTreeMap,
    fmt::Debug,
    sync::Arc,
    time::Duration,
};

use anyhow::Context;
use async_trait::async_trait;
use common::{
    components::ComponentPath,
    execution_context::ExecutionId,
    knobs::{
        FUNCTION_LIMIT_WARNING_RATIO,
        TRANSACTION_MAX_READ_SIZE_BYTES,
        TRANSACTION_MAX_READ_SIZE_ROWS,
    },
    types::{
        ModuleEnvironment,
        StorageUuid,
        UdfIdentifier,
    },
    RequestId,
};
use events::usage::{
    FunctionCallUsageFields,
    InsightReadLimitCall,
    UsageEvent,
    UsageEventLogger,
};
use headers::ContentType;
use parking_lot::Mutex;
use pb::usage::{
    CounterWithComponent as CounterWithComponentProto,
    CounterWithTag as CounterWithTagProto,
    FunctionUsageStats as FunctionUsageStatsProto,
};
use value::{
    heap_size::WithHeapSize,
    sha256::Sha256Digest,
};

mod memory_efficiency;
mod metrics;

pub use memory_efficiency::{
    AggregateCounters,
    AtomicCounter,
    ConcurrentUsageMap,
    ThreadLocalCounter,
};

/// The core usage stats aggregator that is cheaply cloneable
#[derive(Clone, Debug)]
pub struct UsageCounter {
    usage_logger: Arc<dyn UsageEventLogger>,
}

impl UsageCounter {
    pub fn new(usage_logger: Arc<dyn UsageEventLogger>) -> Self {
        Self { usage_logger }
    }

    // Used for tracking storage ingress outside of a user function (e.g. snapshot
    // import/export).
    pub async fn track_independent_storage_ingress_size(
        &self,
        component_path: ComponentPath,
        tag: String,
        ingress_size: u64,
    ) {
        let independent_tracker =
            IndependentStorageCallTracker::new(ExecutionId::new(), self.usage_logger.clone());

        independent_tracker
            .track_storage_ingress_size(component_path, tag, ingress_size)
            .await;
    }

    // Used for tracking storage egress outside of a user function (e.g. snapshot
    // import/export).
    pub async fn track_independent_storage_egress_size(
        &self,
        component_path: ComponentPath,
        tag: String,
        egress_size: u64,
    ) {
        let independent_tracker =
            IndependentStorageCallTracker::new(ExecutionId::new(), self.usage_logger.clone());

        independent_tracker
            .track_storage_egress_size(component_path, tag, egress_size)
            .await;
    }
}

#[derive(Debug, Clone)]
pub struct OccInfo {
    pub table_name: Option<String>,
    pub document_id: Option<String>,
    pub write_source: Option<String>,
    pub retry_count: u64,
}

pub enum CallType {
    Action {
        env: ModuleEnvironment,
        duration: Duration,
        memory_in_mb: u64,
    },
    HttpAction {
        duration: Duration,
        memory_in_mb: u64,

        /// Sha256 of the response body
        response_sha256: Sha256Digest,
    },
    Export,
    CachedQuery,
    UncachedQuery {
        duration: Duration,
        memory_in_mb: u64,
    },
    Mutation {
        duration: Duration,
        memory_in_mb: u64,
        occ_info: Option<OccInfo>,
    },
    Import,
    CloudBackup,
    CloudRestore,
}

impl CallType {
    fn tag(&self) -> &'static str {
        match self {
            Self::Action { .. } => "action",
            Self::Export => "export",
            Self::CachedQuery => "cached_query",
            Self::UncachedQuery { .. } => "uncached_query",
            Self::Mutation { .. } => "mutation",
            Self::HttpAction { .. } => "http_action",
            Self::Import => "import",
            Self::CloudBackup => "cloud_backup",
            Self::CloudRestore => "cloud_restore",
        }
    }

    fn is_occ(&self) -> bool {
        match self {
            Self::Mutation { occ_info, .. } => occ_info.is_some(),
            _ => false,
        }
    }

    fn occ_document_id(&self) -> Option<String> {
        match self {
            Self::Mutation { occ_info, .. } => {
                occ_info.as_ref().and_then(|info| info.document_id.clone())
            },
            _ => None,
        }
    }

    fn occ_table_name(&self) -> Option<String> {
        match self {
            Self::Mutation { occ_info, .. } => {
                occ_info.as_ref().and_then(|info| info.table_name.clone())
            },
            _ => None,
        }
    }

    fn occ_write_source(&self) -> Option<String> {
        match self {
            Self::Mutation { occ_info, .. } => {
                occ_info.as_ref().and_then(|info| info.write_source.clone())
            },
            _ => None,
        }
    }

    fn occ_retry_count(&self) -> Option<u64> {
        match self {
            Self::Mutation { occ_info, .. } => occ_info.as_ref().map(|info| info.retry_count),
            _ => None,
        }
    }

    fn memory_megabytes(&self) -> u64 {
        match self {
            CallType::UncachedQuery { memory_in_mb, .. }
            | CallType::Mutation { memory_in_mb, .. }
            | CallType::Action { memory_in_mb, .. }
            | CallType::HttpAction { memory_in_mb, .. } => *memory_in_mb,
            _ => 0,
        }
    }

    fn duration_millis(&self) -> u64 {
        match self {
            CallType::UncachedQuery { duration, .. }
            | CallType::Mutation { duration, .. }
            | CallType::Action { duration, .. }
            | CallType::HttpAction { duration, .. } => u64::try_from(duration.as_millis())
                .expect("Function was running for over 584 billion years??"),
            _ => 0,
        }
    }

    fn environment(&self) -> String {
        match self {
            CallType::Action { env, .. } => env,
            // All other UDF types, including HTTP actions run on the isolate
            // only.
            _ => &ModuleEnvironment::Isolate,
        }
        .to_string()
    }

    fn response_sha256(&self) -> Option<String> {
        match self {
            CallType::HttpAction {
                response_sha256, ..
            } => Some(response_sha256.as_hex()),
            _ => None,
        }
    }
}

impl UsageCounter {
    pub async fn track_call(
        &self,
        udf_path: UdfIdentifier,
        execution_id: ExecutionId,
        request_id: RequestId,
        call_type: CallType,
        success: bool,
        stats: FunctionUsageStats,
    ) {
        let mut usage_metrics = Vec::new();

        // Because system udfs might cause usage before any data is added by the user,
        // we do not count their calls. We do count their bandwidth.
        let (should_track_calls, udf_id_type) = match &udf_path {
            UdfIdentifier::Function(path) => (!path.udf_path.is_system(), "function"),
            UdfIdentifier::Http(_) => (true, "http"),
            UdfIdentifier::SystemJob(_) => (false, "_system_job"),
        };

        let (component_path, udf_id) = udf_path.clone().into_component_and_udf_path();
        usage_metrics.push(UsageEvent::FunctionCall {
            fields: FunctionCallUsageFields {
                id: execution_id.to_string(),
                request_id: request_id.to_string(),
                status: if success { "success" } else { "failure" }.to_string(),
                component_path,
                udf_id,
                udf_id_type: udf_id_type.to_string(),
                tag: call_type.tag().to_string(),
                memory_megabytes: call_type.memory_megabytes(),
                duration_millis: call_type.duration_millis(),
                environment: call_type.environment(),
                is_tracked: should_track_calls,
                response_sha256: call_type.response_sha256(),
                is_occ: call_type.is_occ(),
                occ_table_name: call_type.occ_table_name(),
                occ_document_id: call_type.occ_document_id(),
                occ_write_source: call_type.occ_write_source(),
                occ_retry_count: call_type.occ_retry_count(),
            },
        });

        // We always track bandwidth, even for system udfs.
        self._track_function_usage(
            udf_path,
            stats,
            execution_id,
            request_id,
            success,
            &mut usage_metrics,
        );
        self.usage_logger.record_async(usage_metrics).await;
    }

    // TODO: The existence of this function is a hack due to shortcuts we have
    // done in Node.js usage tracking. It should only be used by Node.js action
    // callbacks. We should only be using track_call() and never calling this
    // this directly. Otherwise, we will have the usage reflected in the usage
    // stats for billing but not in the UDF execution log counters.
    pub async fn track_function_usage(
        &self,
        udf_path: UdfIdentifier,
        execution_id: ExecutionId,
        request_id: RequestId,
        stats: FunctionUsageStats,
    ) {
        let mut usage_metrics = Vec::new();
        self._track_function_usage(
            udf_path,
            stats,
            execution_id,
            request_id,
            true,
            &mut usage_metrics,
        );
        self.usage_logger.record_async(usage_metrics).await;
    }

    pub fn _track_function_usage(
        &self,
        udf_path: UdfIdentifier,
        stats: FunctionUsageStats,
        execution_id: ExecutionId,
        request_id: RequestId,
        success: bool,
        usage_metrics: &mut Vec<UsageEvent>,
    ) {
        // Merge the storage stats.
        let (_, udf_id) = udf_path.into_component_and_udf_path();
        for ((component_path, storage_api), function_count) in stats.storage_calls {
            usage_metrics.push(UsageEvent::FunctionStorageCalls {
                id: execution_id.to_string(),
                component_path: component_path.serialize(),
                udf_id: udf_id.clone(),
                call: storage_api,
                count: function_count,
            });
        }

        for (component_path, ingress_size) in stats.storage_ingress_size {
            usage_metrics.push(UsageEvent::FunctionStorageBandwidth {
                id: execution_id.to_string(),
                component_path: component_path.serialize(),
                udf_id: udf_id.clone(),
                ingress: ingress_size,
                egress: 0,
            });
        }
        for (component_path, egress_size) in stats.storage_egress_size {
            usage_metrics.push(UsageEvent::FunctionStorageBandwidth {
                id: execution_id.to_string(),
                component_path: component_path.serialize(),
                udf_id: udf_id.clone(),
                ingress: 0,
                egress: egress_size,
            });
        }
        // Merge "by table" bandwidth stats.
        for ((component_path, table_name), ingress_size) in stats.database_ingress_size {
            usage_metrics.push(UsageEvent::DatabaseBandwidth {
                id: execution_id.to_string(),
                request_id: request_id.to_string(),
                component_path: component_path.serialize(),
                udf_id: udf_id.clone(),
                table_name,
                ingress: ingress_size,
                egress: 0,
                egress_rows: 0,
            });
        }
        for ((component_path, table_name), egress_size) in stats.database_egress_size.clone() {
            let rows = stats
                .database_egress_rows
                .get(&(component_path.clone(), table_name.clone()))
                .unwrap_or(&0);
            usage_metrics.push(UsageEvent::DatabaseBandwidth {
                id: execution_id.to_string(),
                request_id: request_id.to_string(),
                component_path: component_path.serialize(),
                udf_id: udf_id.clone(),
                table_name,
                ingress: 0,
                egress: egress_size,
                egress_rows: *rows,
            });
        }

        // Check read limits and add InsightReadLimit event if thresholds are exceeded
        let total_rows: u64 = stats.database_egress_rows.values().sum();
        let total_bytes: u64 = stats.database_egress_size.values().sum();

        let row_threshold =
            (*TRANSACTION_MAX_READ_SIZE_ROWS as f64 * *FUNCTION_LIMIT_WARNING_RATIO) as u64;
        let byte_threshold =
            (*TRANSACTION_MAX_READ_SIZE_BYTES as f64 * *FUNCTION_LIMIT_WARNING_RATIO) as u64;

        let did_exceed_document_threshold = total_rows >= row_threshold;
        let did_exceed_byte_threshold = total_bytes >= byte_threshold;

        if did_exceed_document_threshold || did_exceed_byte_threshold {
            let mut calls = Vec::new();
            let component_path: Option<ComponentPath> =
                match stats.database_egress_rows.first_key_value() {
                    Some(((component_path, _), _)) => Some(component_path.clone()),
                    None => {
                        tracing::error!(
                            "Failed to find component path despite thresholds being exceeded"
                        );
                        None
                    },
                };

            if let Some(component_path) = component_path {
                for ((cp, table_name), egress_rows) in stats.database_egress_rows.into_iter() {
                    let egress = stats
                        .database_egress_size
                        .get(&(cp, table_name.clone()))
                        .copied()
                        .unwrap_or(0);

                    calls.push(InsightReadLimitCall {
                        table_name,
                        bytes_read: egress,
                        documents_read: egress_rows,
                    });
                }

                usage_metrics.push(UsageEvent::InsightReadLimit {
                    id: execution_id.to_string(),
                    request_id: request_id.to_string(),
                    udf_id: udf_id.clone(),
                    component_path: component_path.serialize(),
                    calls,
                    success,
                });
            }
        }

        for ((component_path, table_name), ingress_size) in stats.vector_ingress_size {
            usage_metrics.push(UsageEvent::VectorBandwidth {
                id: execution_id.to_string(),
                component_path: component_path.serialize(),
                udf_id: udf_id.clone(),
                table_name,
                ingress: ingress_size,
                egress: 0,
            });
        }
        for ((component_path, table_name), egress_size) in stats.vector_egress_size {
            usage_metrics.push(UsageEvent::VectorBandwidth {
                id: execution_id.to_string(),
                component_path: component_path.serialize(),
                udf_id: udf_id.clone(),
                table_name,
                ingress: 0,
                egress: egress_size,
            });
        }
    }
}

// We can track storage attributed by UDF or not. This is why unlike database
// and vector search egress/ingress those methods are both on
// FunctionUsageTracker and UsageCounters directly.
#[async_trait]
pub trait StorageUsageTracker: Send + Sync {
    async fn track_storage_call(
        &self,
        component_path: ComponentPath,
        storage_api: &'static str,
        storage_id: StorageUuid,
        content_type: Option<ContentType>,
        sha256: Sha256Digest,
    ) -> Box<dyn StorageCallTracker>;
}

#[async_trait]
pub trait StorageCallTracker: Send + Sync {
    async fn track_storage_ingress_size(
        &self,
        component_path: ComponentPath,
        tag: String,
        ingress_size: u64,
    );
    async fn track_storage_egress_size(
        &self,
        component_path: ComponentPath,
        tag: String,
        egress_size: u64,
    );
}

struct IndependentStorageCallTracker {
    execution_id: ExecutionId,
    usage_logger: Arc<dyn UsageEventLogger>,
}

impl IndependentStorageCallTracker {
    fn new(execution_id: ExecutionId, usage_logger: Arc<dyn UsageEventLogger>) -> Self {
        Self {
            execution_id,
            usage_logger,
        }
    }
}

#[async_trait]
impl StorageCallTracker for IndependentStorageCallTracker {
    async fn track_storage_ingress_size(
        &self,
        component_path: ComponentPath,
        tag: String,
        ingress_size: u64,
    ) {
        metrics::storage::log_storage_ingress_size(ingress_size);
        self.usage_logger
            .record_async(vec![UsageEvent::StorageBandwidth {
                id: self.execution_id.to_string(),
                component_path: component_path.serialize(),
                tag,
                ingress: ingress_size,
                egress: 0,
            }])
            .await;
    }

    async fn track_storage_egress_size(
        &self,
        component_path: ComponentPath,
        tag: String,
        egress_size: u64,
    ) {
        metrics::storage::log_storage_egress_size(egress_size);
        self.usage_logger
            .record_async(vec![UsageEvent::StorageBandwidth {
                id: self.execution_id.to_string(),
                component_path: component_path.serialize(),
                tag,
                ingress: 0,
                egress: egress_size,
            }])
            .await;
    }
}

#[async_trait]
impl StorageUsageTracker for UsageCounter {
    async fn track_storage_call(
        &self,
        component_path: ComponentPath,
        storage_api: &'static str,
        storage_id: StorageUuid,
        content_type: Option<ContentType>,
        sha256: Sha256Digest,
    ) -> Box<dyn StorageCallTracker> {
        let execution_id = ExecutionId::new();
        metrics::storage::log_storage_call();
        self.usage_logger
            .record_async(vec![UsageEvent::StorageCall {
                id: execution_id.to_string(),
                component_path: component_path.serialize(),
                // Ideally we would track the Id<_storage> instead of the StorageUuid
                // but it's a bit annoying for now, so just going with this.
                storage_id: storage_id.to_string(),
                call: storage_api.to_string(),
                content_type: content_type.map(|c| c.to_string()),
                sha256: sha256.as_hex(),
            }])
            .await;

        Box::new(IndependentStorageCallTracker::new(
            execution_id,
            self.usage_logger.clone(),
        ))
    }
}

/// Usage tracker used within a Transaction. Note that this structure does not
/// directly report to the backend global counters and instead only buffers the
/// counters locally. The counters get rolled into the global ones via
/// UsageCounters::track_call() at the end of each UDF. This provides a
/// consistent way to account for usage, where we only bill people for usage
/// that makes it to the UdfExecution log.
#[derive(Debug, Clone)]
pub struct FunctionUsageTracker {
    // TODO: We should ideally not use an Arc<Mutex> here. The best way to achieve
    // this is to move the logic for accounting ingress out of the Committer into
    // the Transaction. Then Transaction can solely own the counters and we can
    // remove clone(). The alternative is for the Committer to take ownership of
    // the usage tracker and then return it, but this will make it complicated if
    // we later decide to charge people for OCC bandwidth.
    state: Arc<Mutex<FunctionUsageStats>>,
}

impl FunctionUsageTracker {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(FunctionUsageStats::default())),
        }
    }

    /// Calculate FunctionUsageStats here
    pub fn gather_user_stats(self) -> FunctionUsageStats {
        self.state.lock().clone()
    }

    /// Adds the given usage stats to the current tracker.
    pub fn add(&self, stats: FunctionUsageStats) {
        self.state.lock().merge(stats);
    }

    // Tracks database usage from write operations (insert/update/delete) for
    // documents that are not in vector indexes. If the document has one or more
    // vectors in a vector index, call `track_vector_ingress_size` instead of
    // this method.
    //
    // You must always check to see if a document is a vector index before
    // calling this method.
    pub fn track_database_ingress_size(
        &self,
        component_path: ComponentPath,
        table_name: String,
        ingress_size: u64,
        skip_logging: bool,
    ) {
        if skip_logging {
            return;
        }

        let mut state = self.state.lock();
        state
            .database_ingress_size
            .mutate_entry_or_default((component_path, table_name), |count| *count += ingress_size);
    }

    pub fn track_database_egress_size(
        &self,
        component_path: ComponentPath,
        table_name: String,
        egress_size: u64,
        skip_logging: bool,
    ) {
        if skip_logging {
            return;
        }

        let mut state = self.state.lock();
        state
            .database_egress_size
            .mutate_entry_or_default((component_path, table_name), |count| *count += egress_size);
    }

    pub fn track_database_egress_rows(
        &self,
        component_path: ComponentPath,
        table_name: String,
        egress_rows: u64,
        skip_logging: bool,
    ) {
        if skip_logging {
            return;
        }

        let mut state = self.state.lock();
        state
            .database_egress_rows
            .mutate_entry_or_default((component_path, table_name), |count| *count += egress_rows);
    }

    // Tracks the vector ingress surcharge and database usage for documents
    // that have one or more vectors in a vector index.
    //
    // If the document does not have a vector in a vector index, call
    // `track_database_ingress_size` instead of this method.
    //
    // Vector bandwidth is a surcharge on vector related bandwidth usage. As a
    // result it counts against both bandwidth ingress and vector ingress.
    // Ingress is a bit trickier than egress because vector ingress needs to be
    // updated whenever the mutated document is in a vector index. To be in a
    // vector index the document must both be in a table with a vector index and
    // have at least one vector that's actually used in the index.
    pub fn track_vector_ingress_size(
        &self,
        component_path: ComponentPath,
        table_name: String,
        ingress_size: u64,
        skip_logging: bool,
    ) {
        if skip_logging {
            return;
        }

        // Note that vector search counts as both database and vector bandwidth
        // per the comment above.
        let mut state = self.state.lock();
        let key = (component_path, table_name);
        state
            .database_ingress_size
            .mutate_entry_or_default(key.clone(), |count| {
                *count += ingress_size;
            });
        state
            .vector_ingress_size
            .mutate_entry_or_default(key, |count| {
                *count += ingress_size;
            });
    }

    // Tracks bandwidth usage from vector searches
    //
    // Vector bandwidth is a surcharge on vector related bandwidth usage. As a
    // result it counts against both bandwidth egress and vector egress. It's an
    // error to increment vector egress without also incrementing database
    // egress. The reverse is not true however, it's totally fine to increment
    // general database egress without incrementing vector egress if the operation
    // is not a vector search.
    //
    // Unlike track_database_ingress_size, this method is explicitly vector related
    // because we should always know that the relevant operation is a vector
    // search. In contrast, for ingress any insert/update/delete could happen to
    // impact a vector index.
    pub fn track_vector_egress_size(
        &self,
        component_path: ComponentPath,
        table_name: String,
        egress_size: u64,
        skip_logging: bool,
    ) {
        if skip_logging {
            return;
        }

        // Note that vector search counts as both database and vector bandwidth
        // per the comment above.
        let mut state = self.state.lock();
        let key = (component_path, table_name);
        state
            .database_egress_size
            .mutate_entry_or_default(key.clone(), |count| *count += egress_size);
        state
            .vector_egress_size
            .mutate_entry_or_default(key, |count| *count += egress_size);
    }
}

// For UDFs, we track storage at the per UDF level, no finer. So we can just
// aggregate over the entire UDF and not worry about sending usage events or
// creating unique execution ids.
// Note: If we want finer-grained breakdown of file bandwidth, we can thread the
// tag through FunctionUsageStats. For now we're just interested in the
// breakdown of file bandwidth from functions vs external sources like snapshot
// export/cloud backups.
#[async_trait]
impl StorageCallTracker for FunctionUsageTracker {
    async fn track_storage_ingress_size(
        &self,
        component_path: ComponentPath,
        _tag: String,
        ingress_size: u64,
    ) {
        let mut state = self.state.lock();
        metrics::storage::log_storage_ingress_size(ingress_size);
        state
            .storage_ingress_size
            .mutate_entry_or_default(component_path, |count| *count += ingress_size);
    }

    async fn track_storage_egress_size(
        &self,
        component_path: ComponentPath,
        _tag: String,
        egress_size: u64,
    ) {
        let mut state = self.state.lock();
        metrics::storage::log_storage_egress_size(egress_size);
        state
            .storage_egress_size
            .mutate_entry_or_default(component_path, |count| *count += egress_size);
    }
}

#[async_trait]
impl StorageUsageTracker for FunctionUsageTracker {
    async fn track_storage_call(
        &self,
        component_path: ComponentPath,
        storage_api: &'static str,
        _storage_id: StorageUuid,
        _content_type: Option<ContentType>,
        _sha256: Sha256Digest,
    ) -> Box<dyn StorageCallTracker> {
        let mut state = self.state.lock();
        metrics::storage::log_storage_call();
        state
            .storage_calls
            .mutate_entry_or_default((component_path, storage_api.to_string()), |count| {
                *count += 1
            });
        Box::new(self.clone())
    }
}

type TableName = String;
type StorageAPI = String;

/// User-facing UDF stats, built
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FunctionUsageStats {
    pub storage_calls: WithHeapSize<BTreeMap<(ComponentPath, StorageAPI), u64>>,
    pub storage_ingress_size: WithHeapSize<BTreeMap<ComponentPath, u64>>,
    pub storage_egress_size: WithHeapSize<BTreeMap<ComponentPath, u64>>,
    pub database_ingress_size: WithHeapSize<BTreeMap<(ComponentPath, TableName), u64>>,
    pub database_egress_size: WithHeapSize<BTreeMap<(ComponentPath, TableName), u64>>,
    pub database_egress_rows: WithHeapSize<BTreeMap<(ComponentPath, TableName), u64>>,
    pub vector_ingress_size: WithHeapSize<BTreeMap<(ComponentPath, TableName), u64>>,
    pub vector_egress_size: WithHeapSize<BTreeMap<(ComponentPath, TableName), u64>>,
}

impl FunctionUsageStats {
    pub fn aggregate(&self) -> AggregatedFunctionUsageStats {
        AggregatedFunctionUsageStats {
            database_read_bytes: self.database_egress_size.values().sum(),
            database_write_bytes: self.database_ingress_size.values().sum(),
            database_read_documents: self.database_egress_rows.values().sum(),
            storage_read_bytes: self.storage_egress_size.values().sum(),
            storage_write_bytes: self.storage_ingress_size.values().sum(),
            vector_index_read_bytes: self.vector_egress_size.values().sum(),
            vector_index_write_bytes: self.vector_ingress_size.values().sum(),
        }
    }

    fn merge(&mut self, other: Self) {
        // Merge the storage stats.
        for (key, function_count) in other.storage_calls {
            self.storage_calls
                .mutate_entry_or_default(key, |count| *count += function_count);
        }
        for (key, ingress_size) in other.storage_ingress_size {
            self.storage_ingress_size
                .mutate_entry_or_default(key, |count| *count += ingress_size);
        }
        for (key, egress_size) in other.storage_egress_size {
            self.storage_egress_size
                .mutate_entry_or_default(key, |count| *count += egress_size);
        }

        // Merge "by table" bandwidth other.
        for (key, ingress_size) in other.database_ingress_size {
            self.database_ingress_size
                .mutate_entry_or_default(key.clone(), |count| *count += ingress_size);
        }
        for (key, egress_size) in other.database_egress_size {
            self.database_egress_size
                .mutate_entry_or_default(key.clone(), |count| *count += egress_size);
        }
        for (key, egress_rows) in other.database_egress_rows {
            self.database_egress_rows
                .mutate_entry_or_default(key.clone(), |count| *count += egress_rows);
        }
        for (key, ingress_size) in other.vector_ingress_size {
            self.vector_ingress_size
                .mutate_entry_or_default(key.clone(), |count| *count += ingress_size);
        }
        for (key, egress_size) in other.vector_egress_size {
            self.vector_egress_size
                .mutate_entry_or_default(key.clone(), |count| *count += egress_size);
        }
    }
}

#[cfg(any(test, feature = "testing"))]
mod usage_arbitrary {
    use proptest::prelude::*;

    use crate::{
        ComponentPath,
        FunctionUsageStats,
        StorageAPI,
        TableName,
        WithHeapSize,
    };

    impl Arbitrary for FunctionUsageStats {
        type Parameters = ();
        type Strategy = BoxedStrategy<Self>;

        fn arbitrary_with((): Self::Parameters) -> Self::Strategy {
            let strategies = (
                proptest::collection::btree_map(
                    any::<(ComponentPath, StorageAPI)>(),
                    0..=1024u64,
                    0..=4,
                )
                .prop_map(WithHeapSize::from),
                proptest::collection::btree_map(any::<ComponentPath>(), 0..=1024u64, 0..=4)
                    .prop_map(WithHeapSize::from),
                proptest::collection::btree_map(any::<ComponentPath>(), 0..=1024u64, 0..=4)
                    .prop_map(WithHeapSize::from),
                proptest::collection::btree_map(
                    any::<(ComponentPath, TableName)>(),
                    0..=1024u64,
                    0..=4,
                )
                .prop_map(WithHeapSize::from),
                proptest::collection::btree_map(
                    any::<(ComponentPath, TableName)>(),
                    0..=1024u64,
                    0..=4,
                )
                .prop_map(WithHeapSize::from),
                proptest::collection::btree_map(
                    any::<(ComponentPath, TableName)>(),
                    0..=1024u64,
                    0..=4,
                )
                .prop_map(WithHeapSize::from),
                proptest::collection::btree_map(
                    any::<(ComponentPath, TableName)>(),
                    0..=1024u64,
                    0..=4,
                )
                .prop_map(WithHeapSize::from),
                proptest::collection::btree_map(
                    any::<(ComponentPath, TableName)>(),
                    0..=1024u64,
                    0..=4,
                )
                .prop_map(WithHeapSize::from),
            );
            strategies
                .prop_map(
                    |(
                        storage_calls,
                        storage_ingress_size,
                        storage_egress_size,
                        database_ingress_size,
                        database_egress_size,
                        database_egress_rows,
                        vector_ingress_size,
                        vector_egress_size,
                    )| FunctionUsageStats {
                        storage_calls,
                        storage_ingress_size,
                        storage_egress_size,
                        database_ingress_size,
                        database_egress_size,
                        database_egress_rows,
                        vector_ingress_size,
                        vector_egress_size,
                    },
                )
                .boxed()
        }
    }
}

fn to_by_tag_count(
    counts: impl Iterator<Item = ((ComponentPath, String), u64)>,
) -> Vec<CounterWithTagProto> {
    counts
        .map(
            |((component_path, table_name), count)| CounterWithTagProto {
                component_path: component_path.serialize(),
                table_name: Some(table_name),
                count: Some(count),
            },
        )
        .collect()
}

fn to_by_component_count(
    counts: impl Iterator<Item = (ComponentPath, u64)>,
) -> Vec<CounterWithComponentProto> {
    counts
        .map(|(component_path, count)| CounterWithComponentProto {
            component_path: component_path.serialize(),
            count: Some(count),
        })
        .collect()
}

fn from_by_tag_count(
    counts: Vec<CounterWithTagProto>,
) -> anyhow::Result<impl Iterator<Item = ((ComponentPath, String), u64)>> {
    let counts: Vec<_> = counts
        .into_iter()
        .map(|c| -> anyhow::Result<_> {
            let component_path = ComponentPath::deserialize(c.component_path.as_deref())?;
            let name = c.table_name.context("Missing `tag` field")?;
            let count = c.count.context("Missing `count` field")?;
            Ok(((component_path, name), count))
        })
        .try_collect()?;
    Ok(counts.into_iter())
}

fn from_by_component_tag_count(
    counts: Vec<CounterWithComponentProto>,
) -> anyhow::Result<impl Iterator<Item = (ComponentPath, u64)>> {
    let counts: Vec<_> = counts
        .into_iter()
        .map(|c| -> anyhow::Result<_> {
            let component_path = ComponentPath::deserialize(c.component_path.as_deref())?;
            let count = c.count.context("Missing `count` field")?;
            Ok((component_path, count))
        })
        .try_collect()?;
    Ok(counts.into_iter())
}

impl From<FunctionUsageStats> for FunctionUsageStatsProto {
    fn from(stats: FunctionUsageStats) -> Self {
        FunctionUsageStatsProto {
            storage_calls: to_by_tag_count(stats.storage_calls.into_iter()),
            storage_ingress_size_by_component: to_by_component_count(
                stats.storage_ingress_size.into_iter(),
            ),
            storage_egress_size_by_component: to_by_component_count(
                stats.storage_egress_size.into_iter(),
            ),
            database_ingress_size: to_by_tag_count(stats.database_ingress_size.into_iter()),
            database_egress_size: to_by_tag_count(stats.database_egress_size.into_iter()),
            database_egress_rows: to_by_tag_count(stats.database_egress_rows.into_iter()),
            vector_ingress_size: to_by_tag_count(stats.vector_ingress_size.into_iter()),
            vector_egress_size: to_by_tag_count(stats.vector_egress_size.into_iter()),
        }
    }
}

impl TryFrom<FunctionUsageStatsProto> for FunctionUsageStats {
    type Error = anyhow::Error;

    fn try_from(stats: FunctionUsageStatsProto) -> anyhow::Result<Self> {
        let storage_calls = from_by_tag_count(stats.storage_calls)?.collect();
        let storage_ingress_size =
            from_by_component_tag_count(stats.storage_ingress_size_by_component)?.collect();
        let storage_egress_size =
            from_by_component_tag_count(stats.storage_egress_size_by_component)?.collect();
        let database_ingress_size = from_by_tag_count(stats.database_ingress_size)?.collect();
        let database_egress_size = from_by_tag_count(stats.database_egress_size)?.collect();
        let database_egress_rows = from_by_tag_count(stats.database_egress_rows)?.collect();
        let vector_ingress_size = from_by_tag_count(stats.vector_ingress_size)?.collect();
        let vector_egress_size = from_by_tag_count(stats.vector_egress_size)?.collect();

        Ok(FunctionUsageStats {
            storage_calls,
            storage_ingress_size,
            storage_egress_size,
            database_ingress_size,
            database_egress_rows,
            database_egress_size,
            vector_ingress_size,
            vector_egress_size,
        })
    }
}

/// User-facing UDF stats, that is logged in the UDF execution log
/// and might be used for debugging purposes.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AggregatedFunctionUsageStats {
    pub database_read_bytes: u64,
    pub database_write_bytes: u64,
    pub database_read_documents: u64,
    pub storage_read_bytes: u64,
    pub storage_write_bytes: u64,
    pub vector_index_read_bytes: u64,
    pub vector_index_write_bytes: u64,
}

/// An optimized usage tracker using lock-free concurrent data structures.
///
/// This implementation reduces lock contention by using `DashMap` for concurrent
/// access to per-table counters. It's designed as an alternative to
/// `FunctionUsageTracker` for scenarios with high concurrency.
///
/// # Performance Characteristics
///
/// - **Reads**: Lock-free, O(1)
/// - **Writes**: Fine-grained locking per key, O(1) average
/// - **Aggregation**: O(n) where n is the number of unique keys
///
/// # Example
///
/// ```ignore
/// let tracker = OptimizedFunctionUsageTracker::new();
///
/// // Track database operations (lock-free)
/// tracker.track_database_ingress(component_path, table_name, 1024);
/// tracker.track_database_egress(component_path, table_name, 512);
///
/// // Get aggregated stats
/// let stats = tracker.into_stats();
/// ```
#[derive(Debug, Clone)]
pub struct OptimizedFunctionUsageTracker {
    /// Storage API call counts by (component, api)
    storage_calls: ConcurrentUsageMap<(ComponentPath, StorageAPI), u64>,
    /// Storage ingress bytes by component
    storage_ingress_size: ConcurrentUsageMap<ComponentPath, u64>,
    /// Storage egress bytes by component
    storage_egress_size: ConcurrentUsageMap<ComponentPath, u64>,
    /// Database ingress bytes by (component, table)
    database_ingress_size: ConcurrentUsageMap<(ComponentPath, TableName), u64>,
    /// Database egress bytes by (component, table)
    database_egress_size: ConcurrentUsageMap<(ComponentPath, TableName), u64>,
    /// Database egress row counts by (component, table)
    database_egress_rows: ConcurrentUsageMap<(ComponentPath, TableName), u64>,
    /// Vector index ingress bytes by (component, table)
    vector_ingress_size: ConcurrentUsageMap<(ComponentPath, TableName), u64>,
    /// Vector index egress bytes by (component, table)
    vector_egress_size: ConcurrentUsageMap<(ComponentPath, TableName), u64>,
}

impl Default for OptimizedFunctionUsageTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl OptimizedFunctionUsageTracker {
    /// Creates a new optimized usage tracker.
    pub fn new() -> Self {
        Self {
            storage_calls: ConcurrentUsageMap::new(),
            storage_ingress_size: ConcurrentUsageMap::new(),
            storage_egress_size: ConcurrentUsageMap::new(),
            database_ingress_size: ConcurrentUsageMap::new(),
            database_egress_size: ConcurrentUsageMap::new(),
            database_egress_rows: ConcurrentUsageMap::new(),
            vector_ingress_size: ConcurrentUsageMap::new(),
            vector_egress_size: ConcurrentUsageMap::new(),
        }
    }

    /// Converts the tracker into a `FunctionUsageStats` for serialization.
    pub fn into_stats(self) -> FunctionUsageStats {
        FunctionUsageStats {
            storage_calls: WithHeapSize::from(self.storage_calls.to_btree_map()),
            storage_ingress_size: WithHeapSize::from(self.storage_ingress_size.to_btree_map()),
            storage_egress_size: WithHeapSize::from(self.storage_egress_size.to_btree_map()),
            database_ingress_size: WithHeapSize::from(self.database_ingress_size.to_btree_map()),
            database_egress_size: WithHeapSize::from(self.database_egress_size.to_btree_map()),
            database_egress_rows: WithHeapSize::from(self.database_egress_rows.to_btree_map()),
            vector_ingress_size: WithHeapSize::from(self.vector_ingress_size.to_btree_map()),
            vector_egress_size: WithHeapSize::from(self.vector_egress_size.to_btree_map()),
        }
    }

    /// Gets a snapshot of the current stats without consuming the tracker.
    pub fn gather_stats(&self) -> FunctionUsageStats {
        FunctionUsageStats {
            storage_calls: WithHeapSize::from(self.storage_calls.to_btree_map()),
            storage_ingress_size: WithHeapSize::from(self.storage_ingress_size.to_btree_map()),
            storage_egress_size: WithHeapSize::from(self.storage_egress_size.to_btree_map()),
            database_ingress_size: WithHeapSize::from(self.database_ingress_size.to_btree_map()),
            database_egress_size: WithHeapSize::from(self.database_egress_size.to_btree_map()),
            database_egress_rows: WithHeapSize::from(self.database_egress_rows.to_btree_map()),
            vector_ingress_size: WithHeapSize::from(self.vector_ingress_size.to_btree_map()),
            vector_egress_size: WithHeapSize::from(self.vector_egress_size.to_btree_map()),
        }
    }

    /// Merges stats from another source into this tracker.
    pub fn merge(&self, stats: FunctionUsageStats) {
        for (key, count) in stats.storage_calls.into_iter() {
            self.storage_calls.increment(key, count);
        }
        for (key, size) in stats.storage_ingress_size.into_iter() {
            self.storage_ingress_size.increment(key, size);
        }
        for (key, size) in stats.storage_egress_size.into_iter() {
            self.storage_egress_size.increment(key, size);
        }
        for (key, size) in stats.database_ingress_size.into_iter() {
            self.database_ingress_size.increment(key, size);
        }
        for (key, size) in stats.database_egress_size.into_iter() {
            self.database_egress_size.increment(key, size);
        }
        for (key, rows) in stats.database_egress_rows.into_iter() {
            self.database_egress_rows.increment(key, rows);
        }
        for (key, size) in stats.vector_ingress_size.into_iter() {
            self.vector_ingress_size.increment(key, size);
        }
        for (key, size) in stats.vector_egress_size.into_iter() {
            self.vector_egress_size.increment(key, size);
        }
    }

    /// Track a storage API call.
    #[inline]
    pub fn track_storage_call(&self, component_path: ComponentPath, api: String) {
        metrics::storage::log_storage_call();
        self.storage_calls.increment((component_path, api), 1);
    }

    /// Track storage ingress bytes.
    #[inline]
    pub fn track_storage_ingress(&self, component_path: ComponentPath, size: u64) {
        metrics::storage::log_storage_ingress_size(size);
        self.storage_ingress_size.increment(component_path, size);
    }

    /// Track storage egress bytes.
    #[inline]
    pub fn track_storage_egress(&self, component_path: ComponentPath, size: u64) {
        metrics::storage::log_storage_egress_size(size);
        self.storage_egress_size.increment(component_path, size);
    }

    /// Track database ingress (write) bytes.
    ///
    /// This method is optimized for high-frequency calls with minimal overhead.
    #[inline]
    pub fn track_database_ingress(
        &self,
        component_path: ComponentPath,
        table_name: String,
        size: u64,
        skip_logging: bool,
    ) {
        if skip_logging {
            return;
        }
        self.database_ingress_size
            .increment((component_path, table_name), size);
    }

    /// Track database egress (read) bytes.
    #[inline]
    pub fn track_database_egress(
        &self,
        component_path: ComponentPath,
        table_name: String,
        size: u64,
        skip_logging: bool,
    ) {
        if skip_logging {
            return;
        }
        self.database_egress_size
            .increment((component_path, table_name), size);
    }

    /// Track database egress row count.
    #[inline]
    pub fn track_database_egress_rows(
        &self,
        component_path: ComponentPath,
        table_name: String,
        rows: u64,
        skip_logging: bool,
    ) {
        if skip_logging {
            return;
        }
        self.database_egress_rows
            .increment((component_path, table_name), rows);
    }

    /// Track vector index ingress bytes.
    ///
    /// Note: This also counts against database ingress as vector storage
    /// is a surcharge on top of regular database bandwidth.
    #[inline]
    pub fn track_vector_ingress(
        &self,
        component_path: ComponentPath,
        table_name: String,
        size: u64,
        skip_logging: bool,
    ) {
        if skip_logging {
            return;
        }
        let key = (component_path, table_name);
        self.database_ingress_size.increment(key.clone(), size);
        self.vector_ingress_size.increment(key, size);
    }

    /// Track vector index egress bytes.
    ///
    /// Note: This also counts against database egress as vector storage
    /// is a surcharge on top of regular database bandwidth.
    #[inline]
    pub fn track_vector_egress(
        &self,
        component_path: ComponentPath,
        table_name: String,
        size: u64,
        skip_logging: bool,
    ) {
        if skip_logging {
            return;
        }
        let key = (component_path, table_name);
        self.database_egress_size.increment(key.clone(), size);
        self.vector_egress_size.increment(key, size);
    }

    /// Get aggregated statistics for the tracker.
    pub fn aggregate(&self) -> AggregatedFunctionUsageStats {
        AggregatedFunctionUsageStats {
            database_read_bytes: self.database_egress_size.sum(),
            database_write_bytes: self.database_ingress_size.sum(),
            database_read_documents: self.database_egress_rows.sum(),
            storage_read_bytes: self.storage_egress_size.sum(),
            storage_write_bytes: self.storage_ingress_size.sum(),
            vector_index_read_bytes: self.vector_egress_size.sum(),
            vector_index_write_bytes: self.vector_ingress_size.sum(),
        }
    }
}

#[cfg(test)]
mod tests {
    use cmd_util::env::env_config;
    use common::components::ComponentPath;
    use proptest::prelude::*;
    use value::testing::assert_roundtrips;

    use super::{
        FunctionUsageStats,
        FunctionUsageStatsProto,
        OptimizedFunctionUsageTracker,
    };

    proptest! {
        #![proptest_config(
            ProptestConfig { cases: 256 * env_config("CONVEX_PROPTEST_MULTIPLIER", 1), failure_persistence: None, ..ProptestConfig::default() }
        )]

        #[test]
        fn test_usage_stats_roundtrips(stats in any::<FunctionUsageStats>()) {
            assert_roundtrips::<FunctionUsageStats, FunctionUsageStatsProto>(stats);
        }
    }

    #[test]
    fn test_optimized_tracker_basic() {
        let tracker = OptimizedFunctionUsageTracker::new();
        let component = ComponentPath::root();

        // Track some database operations
        tracker.track_database_ingress(component.clone(), "users".to_string(), 1024, false);
        tracker.track_database_egress(component.clone(), "users".to_string(), 512, false);
        tracker.track_database_egress_rows(component.clone(), "users".to_string(), 10, false);

        // Verify aggregates
        let stats = tracker.aggregate();
        assert_eq!(stats.database_write_bytes, 1024);
        assert_eq!(stats.database_read_bytes, 512);
        assert_eq!(stats.database_read_documents, 10);
    }

    #[test]
    fn test_optimized_tracker_skip_logging() {
        let tracker = OptimizedFunctionUsageTracker::new();
        let component = ComponentPath::root();

        // Track with skip_logging = true
        tracker.track_database_ingress(component.clone(), "users".to_string(), 1024, true);
        tracker.track_database_egress(component.clone(), "users".to_string(), 512, true);

        // Should be zero because skip_logging was true
        let stats = tracker.aggregate();
        assert_eq!(stats.database_write_bytes, 0);
        assert_eq!(stats.database_read_bytes, 0);
    }

    #[test]
    fn test_optimized_tracker_multiple_tables() {
        let tracker = OptimizedFunctionUsageTracker::new();
        let component = ComponentPath::root();

        // Track operations on multiple tables
        tracker.track_database_ingress(component.clone(), "users".to_string(), 100, false);
        tracker.track_database_ingress(component.clone(), "posts".to_string(), 200, false);
        tracker.track_database_ingress(component.clone(), "users".to_string(), 50, false);

        // Convert to stats and verify
        let stats = tracker.gather_stats();
        assert_eq!(
            *stats.database_ingress_size.get(&(component.clone(), "users".to_string())).unwrap(),
            150
        );
        assert_eq!(
            *stats.database_ingress_size.get(&(component.clone(), "posts".to_string())).unwrap(),
            200
        );
    }

    #[test]
    fn test_optimized_tracker_vector_double_counting() {
        let tracker = OptimizedFunctionUsageTracker::new();
        let component = ComponentPath::root();

        // Vector ingress should count against both database and vector
        tracker.track_vector_ingress(component.clone(), "embeddings".to_string(), 1000, false);

        let stats = tracker.aggregate();
        assert_eq!(stats.database_write_bytes, 1000);
        assert_eq!(stats.vector_index_write_bytes, 1000);
    }

    #[test]
    fn test_optimized_tracker_merge() {
        let tracker = OptimizedFunctionUsageTracker::new();
        let component = ComponentPath::root();

        // Create some stats to merge
        let mut stats = FunctionUsageStats::default();
        stats.database_ingress_size.insert((component.clone(), "users".to_string()), 500);

        // Track directly and merge
        tracker.track_database_ingress(component.clone(), "users".to_string(), 100, false);
        tracker.merge(stats);

        // Should have combined total
        let final_stats = tracker.aggregate();
        assert_eq!(final_stats.database_write_bytes, 600);
    }

    #[test]
    fn test_optimized_tracker_clone() {
        let tracker = OptimizedFunctionUsageTracker::new();
        let component = ComponentPath::root();

        tracker.track_database_ingress(component.clone(), "users".to_string(), 1000, false);

        // Clone should have the same data
        let cloned = tracker.clone();
        let original_stats = tracker.aggregate();
        let cloned_stats = cloned.aggregate();

        assert_eq!(original_stats.database_write_bytes, cloned_stats.database_write_bytes);
    }

    #[test]
    fn test_optimized_tracker_into_stats() {
        let tracker = OptimizedFunctionUsageTracker::new();
        let component = ComponentPath::root();

        tracker.track_database_ingress(component.clone(), "users".to_string(), 100, false);
        tracker.track_storage_call(component.clone(), "get_url".to_string());

        let stats = tracker.into_stats();
        assert_eq!(stats.database_ingress_size.len(), 1);
        assert_eq!(stats.storage_calls.len(), 1);
    }
}
