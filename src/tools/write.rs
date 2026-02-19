use anyhow::Result;
use async_trait::async_trait;
use serde_json::json;

use super::{Tool, ToolResult};

const MAX_WRITE_BYTES: usize = 10 * 1024 * 1024;

pub struct WriteTool;

#[async_trait]
impl Tool for WriteTool {
    fn name(&self) -> &str {
        "write"
    }

    fn description(&self) -> &str {
        "Write content to a file, creating directories as needed. Content must not be empty."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file to write"
                },
                "content": {
                    "type": "string",
                    "description": "The full content to write to the file. Must not be empty."
                }
            },
            "required": ["path", "content"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> Result<ToolResult> {
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required parameter: path"))?;

        let content = args
            .get("content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required parameter: content"))?;

        let content = unescape_if_needed(content);

        if content.trim().is_empty() {
            return Ok(ToolResult {
                output: "Refused: content is empty. Provide the full file content to write."
                    .to_string(),
                is_error: true,
            });
        }

        let content_bytes = content.len();
        if content_bytes > MAX_WRITE_BYTES {
            return Ok(ToolResult {
                output: format!(
                    "Refused: content too large ({} bytes, limit {} bytes).",
                    content_bytes, MAX_WRITE_BYTES
                ),
                is_error: true,
            });
        }

        if let Some(parent) = std::path::Path::new(path).parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        tokio::fs::write(path, &content).await?;

        Ok(ToolResult {
            output: format!("Written {} bytes to {path}", content.len()),
            is_error: false,
        })
    }
}

fn unescape_if_needed(s: &str) -> String {
    if !s.contains('\n') && s.contains("\\n") {
        s.replace("\\n", "\n").replace("\\t", "\t")
    } else {
        s.to_string()
    }
}
