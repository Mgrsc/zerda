use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;
use serde::Deserialize;

use super::{Tool, ToolResult};

const DEFAULT_MAX_RESULTS: u64 = 5;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

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
    backend: Box<dyn DocSearchBackend>,
}

impl SearchDocsTool {
    pub fn try_from_env() -> Option<Self> {
        if let Some(backend) = CloudflareAiSearchBackend::try_from_env() {
            tracing::info!("search_zerda_documents: using Cloudflare AI Search backend");
            return Some(Self {
                backend: Box::new(backend),
            });
        }
        None
    }
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
                        output.push_str(&format!("\n[{}] {} (score: {:.2})", i, src.filename, src.score));
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

struct CloudflareAiSearchBackend {
    client: reqwest::Client,
    account_id: String,
    instance_name: String,
    api_token: String,
}

impl CloudflareAiSearchBackend {
    fn try_from_env() -> Option<Self> {
        let account_id = std::env::var("CF_AI_SEARCH_ACCOUNT_ID").ok().filter(|s| !s.is_empty())?;
        let api_token = std::env::var("CF_AI_SEARCH_API_TOKEN").ok().filter(|s| !s.is_empty())?;
        let instance_name = std::env::var("CF_AI_SEARCH_INSTANCE_NAME").ok().filter(|s| !s.is_empty())?;

        let client = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .build()
            .ok()?;

        Some(Self {
            client,
            account_id,
            instance_name,
            api_token,
        })
    }
}

#[derive(Deserialize)]
struct CfAiSearchResponse {
    result: CfAiSearchResult,
}

#[derive(Deserialize)]
struct CfAiSearchResult {
    response: Option<String>,
    data: Vec<CfAiSearchEntry>,
}

#[derive(Deserialize)]
struct CfAiSearchEntry {
    filename: Option<String>,
    score: Option<f64>,
}

#[async_trait]
impl DocSearchBackend for CloudflareAiSearchBackend {
    async fn search(&self, query: &str, max_results: u64) -> Result<DocSearchResult> {
        let url = format!(
            "https://api.cloudflare.com/client/v4/accounts/{}/autorag/rags/{}/ai-search",
            self.account_id, self.instance_name,
        );

        let body = serde_json::json!({
            "query": query,
            "max_num_results": max_results,
        });

        tracing::debug!(url, %body, "Cloudflare AI Search request");

        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.api_token)
            .json(&body)
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("Cloudflare AI Search returned {status}: {text}");
        }

        let parsed: CfAiSearchResponse = resp.json().await?;

        let sources = parsed
            .result
            .data
            .into_iter()
            .map(|entry| DocSource {
                filename: entry.filename.unwrap_or_else(|| "unknown".to_string()),
                score: entry.score.unwrap_or(0.0),
            })
            .collect();

        Ok(DocSearchResult {
            response: parsed.result.response.unwrap_or_default(),
            sources,
        })
    }
}
