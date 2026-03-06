use std::collections::HashMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use async_trait::async_trait;
use qdrant_client::config::QdrantConfig;
use qdrant_client::qdrant::{
    CreateCollectionBuilder, DeletePointsBuilder, Distance, PointStruct, PointsIdsList,
    QueryPointsBuilder, UpsertPointsBuilder, VectorParamsBuilder,
};
use qdrant_client::{Payload, Qdrant};
use serde::{Deserialize, Serialize};
use tokio::sync::OnceCell;
use tokio::time::sleep;

use super::{Tool, ToolResult};

const DEFAULT_MAX_RESULTS: u64 = 5;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const DEFAULT_EMBEDDING_BASE_URL: &str = "https://api.openai.com/v1";
const INDEX_STATE_FILE: &str = "docs_search_index_state.json";
const INDEX_STATE_VERSION: u32 = 1;
const EMBED_MAX_CHARS: usize = 8000;
const SNIPPET_MAX_CHARS: usize = 260;
const INIT_INDEX_MAX_ATTEMPTS: usize = 8;
const INIT_INDEX_BACKOFF_MS: u64 = 1_000;
const QDRANT_OP_MAX_ATTEMPTS: usize = 6;
const QDRANT_OP_BACKOFF_MS: u64 = 500;
const EMBEDDING_OP_MAX_ATTEMPTS: usize = 4;
const EMBEDDING_OP_BACKOFF_MS: u64 = 700;

#[async_trait]
trait DocSearchBackend: Send + Sync {
    async fn search(&self, query: &str, max_results: u64) -> Result<DocSearchResult>;
}

struct DocSearchResult {
    response: String,
    sources: Vec<DocSource>,
}

struct DocSource {
    filename: String,
    score: f64,
}

pub struct SearchDocsTool {
    backend: std::sync::Arc<dyn DocSearchBackend>,
}

impl SearchDocsTool {
    pub fn try_new(settings: SearchDocsSettings) -> Option<Self> {
        let backend = QdrantDocsBackend::try_new(settings)?;
        backend.spawn_initial_indexing();
        tracing::info!(
            "search_zerda_documents: using Qdrant backend (collection={})",
            backend.collection
        );
        Some(Self { backend })
    }
}

#[derive(Debug, Clone)]
pub struct SearchDocsSettings {
    pub qdrant_url: String,
    pub qdrant_api_key: Option<String>,
    pub collection: String,
    pub docs_root: PathBuf,
    pub embedding_api_key: String,
    pub embedding_base_url: String,
    pub embedding_model: String,
    pub embedding_dim: u64,
}

#[async_trait]
impl Tool for SearchDocsTool {
    fn name(&self) -> &str {
        "search_zerda_documents"
    }

    fn description(&self) -> &str {
        "Search Zerda's own project documentation using semantic search. Use this to find information about Zerda's configuration, commands, features, and architecture."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "The search query describing what you want to find in Zerda's documentation"
                },
                "max_results": {
                    "type": "integer",
                    "description": "Maximum number of source documents to return (1-10, default 5)",
                    "minimum": 1,
                    "maximum": 10
                }
            },
            "required": ["query"]
        })
    }

    fn is_safe_for_concurrent(&self) -> bool {
        true
    }

    async fn execute(&self, args: serde_json::Value) -> Result<ToolResult> {
        let query = args
            .get("query")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'query' parameter"))?;

        if query.is_empty() {
            return Ok(ToolResult {
                output: "Error: query is empty".to_string(),
                is_error: true,
            });
        }

        let max_results = args
            .get("max_results")
            .and_then(|v| v.as_u64())
            .unwrap_or(DEFAULT_MAX_RESULTS)
            .clamp(1, 10);

        tracing::debug!(query, max_results, "search_zerda_documents: executing");

        match self.backend.search(query, max_results).await {
            Ok(result) if result.response.is_empty() && result.sources.is_empty() => {
                Ok(ToolResult {
                    output: "No relevant documents found.".to_string(),
                    is_error: false,
                })
            }
            Ok(result) => {
                let mut output = result.response;
                if !result.sources.is_empty() {
                    output.push_str("\n\n--- Sources ---");
                    for (i, src) in result.sources.iter().enumerate() {
                        output.push_str(&format!(
                            "\n[{}] {} (score: {:.2})",
                            i, src.filename, src.score
                        ));
                    }
                }
                Ok(ToolResult {
                    output,
                    is_error: false,
                })
            }
            Err(e) => {
                tracing::error!("search_zerda_documents failed: {e:#}");
                Ok(ToolResult {
                    output: format!("Search failed: {e}"),
                    is_error: true,
                })
            }
        }
    }
}

#[derive(Clone)]
struct QdrantDocsBackend {
    qdrant: Qdrant,
    collection: String,
    http_client: reqwest::Client,
    embedding_base_url: String,
    embedding_api_key: String,
    embedding_model: String,
    embedding_dim: u64,
    docs_root: PathBuf,
    state_path: PathBuf,
    index_ready: std::sync::Arc<OnceCell<()>>,
}

impl QdrantDocsBackend {
    fn try_new(settings: SearchDocsSettings) -> Option<std::sync::Arc<Self>> {
        let docs_root = settings.docs_root;
        if !docs_root.exists() {
            tracing::warn!(
                "search_zerda_documents: docs root '{}' not found, tool disabled",
                docs_root.display()
            );
            return None;
        }
        let state_path =
            crate::config::resolve_path(crate::config::MEMORY_DIR).join(INDEX_STATE_FILE);

        let mut qdrant_config = QdrantConfig::from_url(&settings.qdrant_url);
        if let Some(api_key) = settings.qdrant_api_key {
            qdrant_config = qdrant_config.api_key(api_key);
        }
        let qdrant = match Qdrant::new(qdrant_config) {
            Ok(client) => client,
            Err(e) => {
                tracing::warn!("search_zerda_documents: failed to create Qdrant client: {e}");
                return None;
            }
        };

        let http_client = match reqwest::Client::builder().timeout(REQUEST_TIMEOUT).build() {
            Ok(client) => client,
            Err(e) => {
                tracing::warn!("search_zerda_documents: failed to create HTTP client: {e}");
                return None;
            }
        };

        Some(std::sync::Arc::new(Self {
            qdrant,
            collection: settings.collection,
            http_client,
            embedding_base_url: if settings.embedding_base_url.trim().is_empty() {
                DEFAULT_EMBEDDING_BASE_URL.to_string()
            } else {
                settings.embedding_base_url
            },
            embedding_api_key: settings.embedding_api_key,
            embedding_model: settings.embedding_model,
            embedding_dim: settings.embedding_dim,
            docs_root,
            state_path,
            index_ready: std::sync::Arc::new(OnceCell::new()),
        }))
    }

    fn spawn_initial_indexing(self: &std::sync::Arc<Self>) {
        let backend = std::sync::Arc::clone(self);
        tokio::spawn(async move {
            if let Err(e) = backend.ensure_indexed_with_retry().await {
                tracing::warn!("search_zerda_documents: initial indexing failed: {e:#}");
            }
        });
    }

    async fn ensure_indexed_with_retry(&self) -> Result<()> {
        self.retry_with_backoff(
            "initial index sync",
            INIT_INDEX_MAX_ATTEMPTS,
            INIT_INDEX_BACKOFF_MS,
            || async { self.ensure_indexed().await },
        )
        .await
    }

    async fn retry_with_backoff<T, F, Fut>(
        &self,
        op_name: &str,
        max_attempts: usize,
        base_backoff_ms: u64,
        mut action: F,
    ) -> Result<T>
    where
        F: FnMut() -> Fut,
        Fut: Future<Output = Result<T>>,
    {
        let mut attempt = 1usize;
        loop {
            match action().await {
                Ok(value) => {
                    if attempt > 1 {
                        tracing::info!(
                            op = op_name,
                            attempt,
                            max_attempts,
                            "search_zerda_documents: retry succeeded"
                        );
                    }
                    return Ok(value);
                }
                Err(err) => {
                    if attempt >= max_attempts {
                        tracing::error!(
                            op = op_name,
                            attempt,
                            max_attempts,
                            error = %err,
                            "search_zerda_documents: retry exhausted"
                        );
                        return Err(err);
                    }
                    let backoff_ms = base_backoff_ms.saturating_mul(1_u64 << (attempt - 1));
                    tracing::warn!(
                        op = op_name,
                        attempt,
                        max_attempts,
                        backoff_ms,
                        error = %err,
                        "search_zerda_documents: operation failed, retrying"
                    );
                    sleep(Duration::from_millis(backoff_ms)).await;
                    attempt += 1;
                }
            }
        }
    }

    async fn ensure_indexed(&self) -> Result<()> {
        self.index_ready
            .get_or_try_init(|| async {
                self.ensure_collection().await?;
                self.sync_docs_to_qdrant().await
            })
            .await
            .map(|_| ())
    }

    async fn ensure_collection(&self) -> Result<()> {
        let exists = self
            .retry_with_backoff(
                "qdrant collection_exists",
                QDRANT_OP_MAX_ATTEMPTS,
                QDRANT_OP_BACKOFF_MS,
                || async {
                    self.qdrant
                        .collection_exists(&self.collection)
                        .await
                        .context("search_zerda_documents: check collection existence")
                },
            )
            .await?;
        if exists {
            return Ok(());
        }
        self.retry_with_backoff(
            "qdrant create_collection",
            QDRANT_OP_MAX_ATTEMPTS,
            QDRANT_OP_BACKOFF_MS,
            || async {
                self.qdrant
                    .create_collection(
                        CreateCollectionBuilder::new(&self.collection).vectors_config(
                            VectorParamsBuilder::new(self.embedding_dim, Distance::Cosine),
                        ),
                    )
                    .await
                    .context("search_zerda_documents: create collection")
            },
        )
        .await?;
        tracing::info!(
            "search_zerda_documents: created Qdrant collection '{}'",
            self.collection
        );
        Ok(())
    }

    async fn sync_docs_to_qdrant(&self) -> Result<()> {
        let current_docs = collect_docs(&self.docs_root)?;
        let previous = load_index_state(&self.state_path).unwrap_or_default();
        let changed_docs = current_docs
            .iter()
            .filter(|doc| previous.files.get(&doc.id) != Some(&doc.state))
            .collect::<Vec<_>>();
        let changed_count = changed_docs.len();
        let removed_ids = previous
            .files
            .keys()
            .filter(|id| !current_docs.iter().any(|doc| doc.id == **id))
            .cloned()
            .collect::<Vec<_>>();

        if !removed_ids.is_empty() {
            let ids = removed_ids
                .into_iter()
                .map(|id| -> qdrant_client::qdrant::PointId { doc_point_id(&id).into() })
                .collect::<Vec<_>>();
            let removed_count = ids.len();
            self.retry_with_backoff(
                "qdrant delete removed docs",
                QDRANT_OP_MAX_ATTEMPTS,
                QDRANT_OP_BACKOFF_MS,
                || async {
                    self.qdrant
                        .delete_points(
                            DeletePointsBuilder::new(&self.collection)
                                .points(PointsIdsList { ids: ids.clone() }),
                        )
                        .await
                        .context("search_zerda_documents: delete removed docs")
                },
            )
            .await?;
            tracing::debug!(
                removed_count,
                collection = %self.collection,
                "search_zerda_documents: removed stale docs from qdrant"
            );
        }

        for doc in changed_docs {
            let content = std::fs::read_to_string(&doc.path)
                .with_context(|| format!("search_zerda_documents: read {}", doc.path.display()))?;
            let vector = self.embed(&content).await?;
            let doc_id = doc.id.clone();
            let doc_content = content.clone();
            let doc_vector = vector.clone();
            self.retry_with_backoff(
                "qdrant upsert doc",
                QDRANT_OP_MAX_ATTEMPTS,
                QDRANT_OP_BACKOFF_MS,
                || async {
                    let payload: Payload = serde_json::json!({
                        "path": doc_id,
                        "content": doc_content,
                    })
                    .try_into()
                    .context("search_zerda_documents: build payload")?;
                    let point =
                        PointStruct::new(doc_point_id(&doc_id), doc_vector.clone(), payload);
                    self.qdrant
                        .upsert_points(UpsertPointsBuilder::new(&self.collection, vec![point]))
                        .await
                        .with_context(|| format!("search_zerda_documents: upsert {doc_id}"))
                },
            )
            .await?;
            tracing::debug!(
                doc_id = %doc.id,
                collection = %self.collection,
                "search_zerda_documents: upserted doc embedding"
            );
        }

        let mut files = HashMap::new();
        for doc in current_docs {
            files.insert(doc.id, doc.state);
        }
        let next_state = DocsIndexState {
            version: INDEX_STATE_VERSION,
            files,
        };
        save_index_state(&self.state_path, &next_state)?;
        tracing::info!(
            changed = changed_count,
            total = next_state.files.len(),
            "search_zerda_documents: docs index synced"
        );
        Ok(())
    }

    async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let url = format!(
            "{}/embeddings",
            self.embedding_base_url.trim_end_matches('/')
        );
        let input = truncate_chars(text, EMBED_MAX_CHARS);
        self.retry_with_backoff(
            "embedding request",
            EMBEDDING_OP_MAX_ATTEMPTS,
            EMBEDDING_OP_BACKOFF_MS,
            || async {
                let body = serde_json::json!({
                    "model": self.embedding_model,
                    "input": input.clone(),
                    "dimensions": self.embedding_dim,
                });
                let resp = self
                    .http_client
                    .post(&url)
                    .header(
                        "Authorization",
                        format!("Bearer {}", self.embedding_api_key),
                    )
                    .json(&body)
                    .send()
                    .await
                    .context("search_zerda_documents: embedding request failed")?;
                let status = resp.status();
                if !status.is_success() {
                    let text = resp.text().await.unwrap_or_default();
                    anyhow::bail!(
                        "search_zerda_documents: embedding API returned {status}: {text}"
                    );
                }
                let parsed: EmbeddingResponse = resp
                    .json()
                    .await
                    .context("search_zerda_documents: parse embedding response")?;
                parsed
                    .data
                    .into_iter()
                    .next()
                    .map(|d| d.embedding)
                    .ok_or_else(|| {
                        anyhow::anyhow!("search_zerda_documents: empty embedding response")
                    })
            },
        )
        .await
    }
}

#[async_trait]
impl DocSearchBackend for QdrantDocsBackend {
    async fn search(&self, query: &str, max_results: u64) -> Result<DocSearchResult> {
        self.ensure_indexed_with_retry().await?;
        let vector = self.embed(query).await?;
        let response = self
            .retry_with_backoff(
                "qdrant query docs",
                QDRANT_OP_MAX_ATTEMPTS,
                QDRANT_OP_BACKOFF_MS,
                || async {
                    self.qdrant
                        .query(
                            QueryPointsBuilder::new(&self.collection)
                                .query(vector.clone())
                                .limit(max_results)
                                .with_payload(true),
                        )
                        .await
                        .context("search_zerda_documents: query qdrant")
                },
            )
            .await?;

        let mut blocks = Vec::new();
        let mut sources = Vec::new();
        for (idx, point) in response.result.into_iter().enumerate() {
            let payload = &point.payload;
            let path = payload
                .get("path")
                .and_then(|v| v.as_str())
                .map_or("unknown", |v| v);
            let content = payload
                .get("content")
                .and_then(|v| v.as_str())
                .map_or("", |v| v);
            let snippet = build_snippet(content, query);
            blocks.push(format!("[{}] {}\n{}", idx + 1, path, snippet));
            sources.push(DocSource {
                filename: path.to_string(),
                score: f64::from(point.score),
            });
        }

        Ok(DocSearchResult {
            response: blocks.join("\n\n"),
            sources,
        })
    }
}

#[derive(Debug, Deserialize)]
struct EmbeddingResponse {
    data: Vec<EmbeddingData>,
}

#[derive(Debug, Deserialize)]
struct EmbeddingData {
    embedding: Vec<f32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
struct DocsIndexState {
    version: u32,
    files: HashMap<String, DocState>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct DocState {
    modified_unix: i64,
    size: u64,
}

#[derive(Debug, Clone)]
struct DocEntry {
    id: String,
    path: PathBuf,
    state: DocState,
}

fn collect_docs(root: &Path) -> Result<Vec<DocEntry>> {
    let mut docs = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir)
            .with_context(|| format!("search_zerda_documents: read dir {}", dir.display()))?
        {
            let entry = entry.with_context(|| {
                format!(
                    "search_zerda_documents: read dir entry in {}",
                    dir.display()
                )
            })?;
            let path = entry.path();
            let file_type = entry.file_type().with_context(|| {
                format!("search_zerda_documents: file type for {}", path.display())
            })?;
            if file_type.is_dir() {
                stack.push(path);
                continue;
            }
            if !file_type.is_file() {
                continue;
            }
            if path.extension().and_then(|v| v.to_str()) != Some("md") {
                continue;
            }
            let meta = entry.metadata().with_context(|| {
                format!("search_zerda_documents: metadata for {}", path.display())
            })?;
            let modified_unix = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or_default();
            let rel = path
                .strip_prefix(root)
                .with_context(|| {
                    format!("search_zerda_documents: strip prefix {}", path.display())
                })?
                .to_string_lossy()
                .replace('\\', "/");
            docs.push(DocEntry {
                id: rel,
                path,
                state: DocState {
                    modified_unix,
                    size: meta.len(),
                },
            });
        }
    }
    docs.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(docs)
}

fn load_index_state(path: &Path) -> Option<DocsIndexState> {
    let text = std::fs::read_to_string(path).ok()?;
    let parsed: DocsIndexState = serde_json::from_str(&text).ok()?;
    if parsed.version == INDEX_STATE_VERSION {
        Some(parsed)
    } else {
        None
    }
}

fn save_index_state(path: &Path, state: &DocsIndexState) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("search_zerda_documents: mkdir {}", parent.display()))?;
    }
    let text =
        serde_json::to_string(state).context("search_zerda_documents: encode index state")?;
    std::fs::write(path, text)
        .with_context(|| format!("search_zerda_documents: write {}", path.display()))?;
    Ok(())
}

fn build_snippet(content: &str, query: &str) -> String {
    let normalized_query = query.trim().to_lowercase();
    if normalized_query.is_empty() {
        return truncate_chars(content, SNIPPET_MAX_CHARS).replace('\n', " ");
    }
    if let Some(line) = content
        .lines()
        .find(|line| line.to_lowercase().contains(&normalized_query))
    {
        return truncate_chars(line, SNIPPET_MAX_CHARS).replace('\n', " ");
    }
    truncate_chars(content, SNIPPET_MAX_CHARS).replace('\n', " ")
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    text.chars().take(max_chars).collect::<String>()
}

fn doc_point_id(doc_id: &str) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for b in doc_id.as_bytes() {
        hash ^= u64::from(*b);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}
