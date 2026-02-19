use anyhow::Result;
use async_trait::async_trait;
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncSeekExt};

use super::{Tool, ToolResult};

const MAX_READ_BYTES: u64 = 10 * 1024 * 1024;
const PREVIEW_HEAD_BYTES: usize = 32 * 1024;
const PREVIEW_TAIL_BYTES: usize = 32 * 1024;

pub struct ReadTool;

#[async_trait]
impl Tool for ReadTool {
    fn name(&self) -> &str {
        "read"
    }

    fn description(&self) -> &str {
        "Read the contents of a file"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file to read"
                }
            },
            "required": ["path"]
        })
    }

    fn is_safe_for_concurrent(&self) -> bool {
        true
    }

    async fn execute(&self, args: serde_json::Value) -> Result<ToolResult> {
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required parameter: path"))?;

        let metadata = match tokio::fs::metadata(path).await {
            Ok(m) => m,
            Err(e) => {
                return Ok(ToolResult {
                    output: format!("Failed to read metadata for {path}: {e}"),
                    is_error: true,
                });
            }
        };

        if !metadata.is_file() {
            return Ok(ToolResult {
                output: format!("Refused to read {path}: not a regular file"),
                is_error: true,
            });
        }

        let file_size = metadata.len();
        if file_size > MAX_READ_BYTES {
            match read_preview(path, file_size).await {
                Ok(preview) => {
                    return Ok(ToolResult {
                        output: preview,
                        is_error: false,
                    });
                }
                Err(e) => {
                    return Ok(ToolResult {
                        output: format!("Failed to read preview from {path}: {e}"),
                        is_error: true,
                    });
                }
            }
        }

        match tokio::fs::read_to_string(path).await {
            Ok(contents) => Ok(ToolResult {
                output: contents,
                is_error: false,
            }),
            Err(e) => Ok(ToolResult {
                output: format!("Failed to read {path}: {e}"),
                is_error: true,
            }),
        }
    }
}

async fn read_preview(path: &str, file_size: u64) -> Result<String> {
    let mut file = tokio::fs::File::open(path).await?;

    let mut head = vec![0u8; PREVIEW_HEAD_BYTES.min(file_size as usize)];
    if !head.is_empty() {
        file.read_exact(&mut head).await?;
    }

    let tail_len = PREVIEW_TAIL_BYTES.min(file_size as usize);
    let mut tail = vec![0u8; tail_len];
    if tail_len > 0 {
        let start = file_size.saturating_sub(tail_len as u64);
        file.seek(std::io::SeekFrom::Start(start)).await?;
        file.read_exact(&mut tail).await?;
    }

    let head_text = String::from_utf8_lossy(&head);
    let tail_text = String::from_utf8_lossy(&tail);

    Ok(format!(
        "File is too large ({file_size} bytes, limit {MAX_READ_BYTES} bytes). Returning truncated preview.\n--- HEAD ({}) ---\n{}\n--- TAIL ({}) ---\n{}",
        head.len(),
        head_text,
        tail.len(),
        tail_text
    ))
}
