# Extending and Integrating Convex

**Part 6 of the Convex Backend Technical Series**

**Analysis Commit:** [`482df8a`](https://github.com/get-convex/convex-backend/tree/482df8a0e1288c9410be8241deb06b09d7ff70b0)

---

## What You'll Learn

- Storage backend abstraction and implementations
- External integrations (Fivetran, S3)
- HTTP Actions for custom endpoints
- The deployment configuration system
- Extension points in the architecture

---

## Introduction

No system exists in isolation. Convex is designed to integrate with your existing infrastructure—file storage, data pipelines, authentication providers, and more. In this post, we'll explore the extension points that make this possible.

## Storage Backend Abstraction

The `Storage` trait allows different file storage backends:

```rust
// Source: crates/storage/src/lib.rs (lines 147-200)
// https://github.com/get-convex/convex-backend/blob/482df8a0e1288c9410be8241deb06b09d7ff70b0/crates/storage/src/lib.rs#L147-L200

#[async_trait]
pub trait Storage: Send + Sync + Debug {
    /// Start a buffered upload
    async fn start_upload(&self) -> anyhow::Result<Box<BufferedUpload>>;

    /// Client-driven multipart upload
    async fn start_client_driven_upload(&self)
        -> anyhow::Result<ClientDrivenUploadToken>;

    /// Upload a part
    async fn upload_part(
        &self,
        token: ClientDrivenUploadToken,
        part_number: u16,
        part: Bytes,
    ) -> anyhow::Result<ClientDrivenUploadPartToken>;

    /// Complete multipart upload
    async fn finish_client_driven_upload(
        &self,
        token: ClientDrivenUploadToken,
        part_tokens: Vec<ClientDrivenUploadPartToken>,
    ) -> anyhow::Result<ObjectKey>;

    /// Generate signed URL for download
    async fn signed_url(
        &self,
        key: ObjectKey,
        expires_in: Duration
    ) -> anyhow::Result<String>;

    /// Generate presigned URL for upload
    async fn presigned_upload_url(
        &self,
        expires_in: Duration
    ) -> anyhow::Result<(ObjectKey, String)>;

    /// Get object metadata
    async fn get_object_attributes(
        &self,
        key: &ObjectKey
    ) -> anyhow::Result<Option<ObjectAttributes>>;
}
```

### Local Storage Implementation

For development:

```rust
// crates/storage/src/local_dir.rs

pub struct LocalDirStorage {
    base_path: PathBuf,
}

#[async_trait]
impl Storage for LocalDirStorage {
    async fn signed_url(&self, key: ObjectKey, _expires: Duration)
        -> anyhow::Result<String> {
        // Return file:// URL for local dev
        let path = self.base_path.join(key.to_string());
        Ok(format!("file://{}", path.display()))
    }

    async fn start_upload(&self) -> anyhow::Result<Box<BufferedUpload>> {
        let temp_path = self.base_path.join(Uuid::new_v4().to_string());
        Ok(Box::new(LocalBufferedUpload::new(temp_path)))
    }
}
```

### S3 Storage Implementation

For production:

```rust
// Source: crates/aws_s3/src/storage.rs
// https://github.com/get-convex/convex-backend/blob/482df8a0e1288c9410be8241deb06b09d7ff70b0/crates/aws_s3/src/storage.rs

pub struct S3Storage {
    client: aws_sdk_s3::Client,
    bucket: String,
    region: String,
}

#[async_trait]
impl Storage for S3Storage {
    async fn signed_url(&self, key: ObjectKey, expires: Duration)
        -> anyhow::Result<String> {
        let presigning_config = PresigningConfig::expires_in(expires)?;

        let request = self.client
            .get_object()
            .bucket(&self.bucket)
            .key(key.to_string())
            .presigned(presigning_config)
            .await?;

        Ok(request.uri().to_string())
    }
}
```

## Fivetran Integration

Convex provides both source and destination connectors for Fivetran.

### Fivetran Source (Export)

Export data from Convex to data warehouses:

```rust
// Source: crates/fivetran_source/src/main.rs
// https://github.com/get-convex/convex-backend/blob/482df8a0e1288c9410be8241deb06b09d7ff70b0/crates/fivetran_source/src/main.rs

pub struct FivetranSource {
    convex_client: ConvexClient,
}

impl FivetranSource {
    /// Stream changes since last sync
    pub async fn get_changes(
        &self,
        state: &SyncState,
    ) -> anyhow::Result<ChangeStream> {
        let changes = self.convex_client
            .export_changes_since(state.last_timestamp)
            .await?;

        Ok(ChangeStream::new(changes))
    }
}
```

### Fivetran Destination (Import)

Import data into Convex from external sources:

```rust
// Source: crates/fivetran_destination/src/main.rs
// https://github.com/get-convex/convex-backend/blob/482df8a0e1288c9410be8241deb06b09d7ff70b0/crates/fivetran_destination/src/main.rs

pub struct FivetranDestination {
    convex_client: ConvexClient,
}

impl FivetranDestination {
    /// Import records from Fivetran
    pub async fn write_batch(
        &self,
        table: &str,
        records: Vec<Record>,
    ) -> anyhow::Result<WriteResult> {
        for record in records {
            match record.operation {
                Operation::Upsert => {
                    self.convex_client.upsert(table, record.data).await?;
                }
                Operation::Delete => {
                    self.convex_client.delete(table, record.id).await?;
                }
            }
        }

        Ok(WriteResult::success())
    }
}
```

## HTTP Actions

HTTP Actions let you create custom HTTP endpoints:

```rust
// Source: crates/local_backend/src/http_actions.rs
// https://github.com/get-convex/convex-backend/blob/482df8a0e1288c9410be8241deb06b09d7ff70b0/crates/local_backend/src/http_actions.rs

pub async fn http_action_handler(
    State(state): State<RouterState>,
    request: Request<Body>,
) -> Response<Body> {
    // Extract action path from URL
    let path = extract_action_path(&request);

    // Build HTTP action request
    let action_request = HttpActionRequest {
        method: request.method().clone(),
        url: request.uri().to_string(),
        headers: extract_headers(&request),
        body: extract_body(request).await?,
    };

    // Execute the action
    let result = state.api
        .execute_http_action(
            &host,
            request_id,
            action_request,
            identity,
            caller,
            response_streamer,
        )
        .await;

    // Convert to HTTP response
    match result {
        Ok(response) => response.into_http_response(),
        Err(e) => error_response(e),
    }
}
```

### User-side HTTP Action

```typescript
// convex/http.ts
import { httpRouter } from "convex/server";
import { httpAction } from "./_generated/server";

const http = httpRouter();

http.route({
  path: "/webhook",
  method: "POST",
  handler: httpAction(async (ctx, request) => {
    const body = await request.json();

    // Process webhook
    await ctx.runMutation(internal.webhooks.process, {
      payload: body,
    });

    return new Response("OK", { status: 200 });
  }),
});

export default http;
```

## Authentication Integration

Convex integrates with authentication providers:

```rust
// Source: crates/authentication/src/lib.rs
// https://github.com/get-convex/convex-backend/blob/482df8a0e1288c9410be8241deb06b09d7ff70b0/crates/authentication/src/lib.rs

pub struct AuthConfig {
    // Auth0, Clerk, etc.
    pub providers: Vec<AuthProvider>,

    // Custom JWT validation
    pub jwt_config: Option<JwtConfig>,
}

pub enum AuthProvider {
    Auth0 { domain: String, client_id: String },
    Clerk { publishable_key: String },
    Custom { issuer: String, jwks_url: String },
}

pub async fn validate_token(
    config: &AuthConfig,
    token: &str,
) -> anyhow::Result<Identity> {
    // Decode JWT
    let claims = decode_jwt(token)?;

    // Find matching provider
    let provider = config.providers
        .iter()
        .find(|p| p.matches_issuer(&claims.iss))
        .ok_or(AuthError::UnknownProvider)?;

    // Validate with provider
    provider.validate(&claims).await?;

    Ok(Identity::from_claims(claims))
}
```

## Deployment Configuration

The deploy config system manages application settings:

```rust
// Source: crates/application/src/deploy_config.rs
// https://github.com/get-convex/convex-backend/blob/482df8a0e1288c9410be8241deb06b09d7ff70b0/crates/application/src/deploy_config.rs

pub struct DeployConfig {
    // Function definitions
    pub functions: BTreeMap<FunctionPath, FunctionConfig>,

    // Schema definitions
    pub schema: Option<SchemaDefinition>,

    // Auth configuration
    pub auth: AuthConfig,

    // Cron jobs
    pub crons: Vec<CronJob>,

    // Environment variables
    pub env_vars: BTreeMap<String, String>,
}

pub struct FunctionConfig {
    pub function_type: FunctionType,
    pub visibility: Visibility,
    pub args_validator: Option<Validator>,
    pub returns_validator: Option<Validator>,
}
```

### Push Deployment

```rust
pub async fn push_config(
    &self,
    config: DeployConfig,
) -> anyhow::Result<PushResult> {
    // Validate config
    self.validate_config(&config)?;

    // Create transaction
    let mut tx = self.database.begin().await?;

    // Update functions
    for (path, func_config) in config.functions {
        tx.update_function(path, func_config).await?;
    }

    // Update schema
    if let Some(schema) = config.schema {
        tx.update_schema(schema).await?;
    }

    // Commit
    tx.commit().await?;

    Ok(PushResult::success())
}
```

## Snapshot Import/Export

For data migration and backup:

### Export

```rust
// Source: crates/application/src/snapshot_export/mod.rs
// https://github.com/get-convex/convex-backend/blob/482df8a0e1288c9410be8241deb06b09d7ff70b0/crates/application/src/snapshot_export/mod.rs

pub struct SnapshotExporter {
    database: Database<RT>,
    storage: Arc<dyn Storage>,
}

impl SnapshotExporter {
    pub async fn export(&self, format: ExportFormat)
        -> anyhow::Result<ExportHandle> {
        // Create snapshot at current timestamp
        let snapshot = self.database.latest_snapshot();

        // Stream tables to storage
        for table in snapshot.tables() {
            let documents = snapshot.scan_table(table).await?;

            match format {
                ExportFormat::JsonLines => {
                    self.write_jsonl(table, documents).await?;
                }
                ExportFormat::Parquet => {
                    self.write_parquet(table, documents).await?;
                }
            }
        }

        Ok(ExportHandle::new(self.storage.clone()))
    }
}
```

### Import

```rust
// Source: crates/application/src/snapshot_import/mod.rs
// https://github.com/get-convex/convex-backend/blob/482df8a0e1288c9410be8241deb06b09d7ff70b0/crates/application/src/snapshot_import/mod.rs

pub struct SnapshotImporter {
    database: Database<RT>,
}

impl SnapshotImporter {
    pub async fn import(
        &self,
        source: ImportSource,
        mode: ImportMode,
    ) -> anyhow::Result<ImportResult> {
        match mode {
            ImportMode::Replace => {
                // Clear existing data
                self.clear_tables().await?;
            }
            ImportMode::Append => {
                // Keep existing data
            }
        }

        // Stream documents from source
        let mut reader = source.open().await?;

        while let Some(batch) = reader.next_batch().await? {
            self.import_batch(batch).await?;
        }

        Ok(ImportResult::success())
    }
}
```

## Environment Variables

Securely manage environment-specific configuration:

```rust
// Source: crates/application/src/environment_variables.rs
// https://github.com/get-convex/convex-backend/blob/482df8a0e1288c9410be8241deb06b09d7ff70b0/crates/application/src/environment_variables.rs

pub struct EnvironmentVariables {
    // Encrypted at rest
    variables: BTreeMap<String, EncryptedValue>,
}

impl EnvironmentVariables {
    pub fn get(&self, key: &str, decryptor: &Decryptor)
        -> anyhow::Result<Option<String>> {
        match self.variables.get(key) {
            Some(encrypted) => {
                let decrypted = decryptor.decrypt(encrypted)?;
                Ok(Some(decrypted))
            }
            None => Ok(None),
        }
    }
}
```

## Log Streaming

Stream logs to external services:

```rust
// Source: crates/log_streaming/src/lib.rs
// https://github.com/get-convex/convex-backend/blob/482df8a0e1288c9410be8241deb06b09d7ff70b0/crates/log_streaming/src/lib.rs

pub enum LogSink {
    Datadog {
        api_key: String,
        site: String,
    },
    Axiom {
        api_token: String,
        dataset: String,
    },
    Webhook {
        url: String,
        headers: BTreeMap<String, String>,
    },
}

pub async fn stream_logs(
    sink: &LogSink,
    logs: Vec<LogEntry>,
) -> anyhow::Result<()> {
    match sink {
        LogSink::Datadog { api_key, site } => {
            let client = DatadogClient::new(api_key, site);
            client.send_logs(logs).await?;
        }
        LogSink::Axiom { api_token, dataset } => {
            let client = AxiomClient::new(api_token);
            client.ingest(dataset, logs).await?;
        }
        LogSink::Webhook { url, headers } => {
            let client = reqwest::Client::new();
            client.post(url)
                .headers(headers.clone())
                .json(&logs)
                .send()
                .await?;
        }
    }

    Ok(())
}
```

## Extension Points Summary

| Extension | Trait/Type | Purpose |
|-----------|-----------|---------|
| Storage | `Storage` | File storage backends |
| Persistence | `Persistence` | Database backends |
| Authentication | `AuthProvider` | Identity providers |
| Log Streaming | `LogSink` | External log services |
| HTTP Actions | `httpAction` | Custom endpoints |
| Scheduled Jobs | `CronJob` | Background tasks |

## Key Takeaways

1. **Trait-based abstractions** - Storage, persistence, auth are all pluggable

2. **First-class integrations** - Fivetran, S3, auth providers built-in

3. **HTTP Actions** - Full control over HTTP request/response

4. **Import/Export** - Data portability is a priority

5. **Secure configuration** - Encrypted env vars, proper secrets management

## What's Next?

In the final post, we'll analyze Convex's performance characteristics and identify optimization opportunities.

---

## References

- [Storage Implementation](https://github.com/get-convex/convex-backend/blob/482df8a0e1288c9410be8241deb06b09d7ff70b0/crates/storage/src/lib.rs)
- [HTTP Actions](https://github.com/get-convex/convex-backend/blob/482df8a0e1288c9410be8241deb06b09d7ff70b0/crates/local_backend/src/http_actions.rs)
- [Log Streaming](https://github.com/get-convex/convex-backend/blob/482df8a0e1288c9410be8241deb06b09d7ff70b0/crates/log_streaming/src/lib.rs)

---

*Next: [Part 7 - Performance Analysis and Optimization Opportunities](07-performance-analysis.md)*
