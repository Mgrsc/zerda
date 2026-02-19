use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use serde_json::json;

use super::{Tool, ToolResult};
use crate::memory::Memory;

pub struct MemoryTool {
    memory: Arc<Memory>,
    max_read_chars: usize,
}

impl MemoryTool {
    pub fn new(memory: Arc<Memory>, max_read_chars: usize) -> Self {
        Self {
            memory,
            max_read_chars,
        }
    }
}

#[async_trait]
impl Tool for MemoryTool {
    fn name(&self) -> &str {
        "memory"
    }

    fn description(&self) -> &str {
        "Read or append to long-term memory (MEMORY.md). Use action 'read' to recall past context, 'append' to record new information."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["read", "append"],
                    "description": "Action to perform: 'read' to retrieve memory, 'append' to add new content"
                },
                "content": {
                    "type": "string",
                    "description": "Content to append (required for 'append' action)"
                }
            },
            "required": ["action"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> Result<ToolResult> {
        let action = args
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("read");

        match action {
            "read" => {
                let content = self
                    .memory
                    .read_memory(self.max_read_chars)
                    .unwrap_or_else(|| "(empty)".to_string());
                Ok(ToolResult {
                    output: content,
                    is_error: false,
                })
            }
            "append" => {
                let content = args.get("content").and_then(|v| v.as_str()).unwrap_or("");
                if content.is_empty() {
                    return Ok(ToolResult {
                        output: "Error: 'content' is required for append action.".to_string(),
                        is_error: true,
                    });
                }
                self.memory.append_memory(content)?;
                Ok(ToolResult {
                    output: "Memory appended successfully.".to_string(),
                    is_error: false,
                })
            }
            other => Ok(ToolResult {
                output: format!("Unknown action: {other}. Use 'read' or 'append'."),
                is_error: true,
            }),
        }
    }
}
