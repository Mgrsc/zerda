use std::fmt::Write as _;
use std::sync::{Arc, Mutex};

use anyhow::Result;
use async_trait::async_trait;
use serde_json::json;

use super::{Tool, ToolResult};

struct TodoItem {
    id: usize,
    text: String,
    done: bool,
}

struct TodoState {
    items: Vec<TodoItem>,
    next_id: usize,
}

pub struct TodoTool {
    state: Arc<Mutex<TodoState>>,
}

impl TodoTool {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(TodoState {
                items: Vec::new(),
                next_id: 1,
            })),
        }
    }
}

#[async_trait]
impl Tool for TodoTool {
    fn name(&self) -> &str {
        "todo"
    }

    fn description(&self) -> &str {
        "Use this tool for multi-step or non-trivial tasks to keep a live plan. \
         Typical flow: list current tasks, add missing tasks, edit when scope changes, \
         mark tasks done as progress happens, and clear when everything is completed. \
         Supports add, edit, done, list, and clear."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["add", "edit", "done", "list", "clear"],
                    "description": "Action to perform: add=create task, edit=update task text, done=mark completed, list=show all tasks, clear=remove all tasks"
                },
                "id": {
                    "type": "integer",
                    "description": "Task ID (required for edit and done)"
                },
                "text": {
                    "type": "string",
                    "description": "Task text (required for add and edit)"
                }
            },
            "required": ["action"],
            "allOf": [
                {
                    "if": {
                        "properties": { "action": { "const": "add" } },
                        "required": ["action"]
                    },
                    "then": { "required": ["text"] }
                },
                {
                    "if": {
                        "properties": { "action": { "const": "edit" } },
                        "required": ["action"]
                    },
                    "then": { "required": ["id", "text"] }
                },
                {
                    "if": {
                        "properties": { "action": { "const": "done" } },
                        "required": ["action"]
                    },
                    "then": { "required": ["id"] }
                }
            ]
        })
    }

    fn is_safe_for_concurrent(&self) -> bool {
        false
    }

    async fn execute(&self, args: serde_json::Value) -> Result<ToolResult> {
        let action = args
            .get("action")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required parameter: action"))?;

        let mut state = self.state.lock().map_err(|e| anyhow::anyhow!("{e}"))?;

        match action {
            "add" => {
                let text = args
                    .get("text")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("Missing required parameter: text"))?;
                let id = state.next_id;
                state.next_id += 1;
                state.items.push(TodoItem {
                    id,
                    text: text.to_string(),
                    done: false,
                });
                Ok(ToolResult {
                    output: format!("Added task #{id}: {text}"),
                    is_error: false,
                })
            }
            "edit" => {
                let id = args
                    .get("id")
                    .and_then(|v| v.as_u64())
                    .ok_or_else(|| anyhow::anyhow!("Missing required parameter: id"))?
                    as usize;
                let text = args
                    .get("text")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("Missing required parameter: text"))?;
                if let Some(item) = state.items.iter_mut().find(|i| i.id == id) {
                    item.text = text.to_string();
                    Ok(ToolResult {
                        output: format!("Updated task #{id}: {text}"),
                        is_error: false,
                    })
                } else {
                    Ok(ToolResult {
                        output: format!("Task #{id} not found"),
                        is_error: true,
                    })
                }
            }
            "done" => {
                let id = args
                    .get("id")
                    .and_then(|v| v.as_u64())
                    .ok_or_else(|| anyhow::anyhow!("Missing required parameter: id"))?
                    as usize;
                if let Some(item) = state.items.iter_mut().find(|i| i.id == id) {
                    item.done = true;
                    Ok(ToolResult {
                        output: format!("Marked task #{id} as done"),
                        is_error: false,
                    })
                } else {
                    Ok(ToolResult {
                        output: format!("Task #{id} not found"),
                        is_error: true,
                    })
                }
            }
            "list" => {
                if state.items.is_empty() {
                    return Ok(ToolResult {
                        output: "No tasks.".to_string(),
                        is_error: false,
                    });
                }
                let mut out = String::new();
                for item in &state.items {
                    let status = if item.done { "x" } else { " " };
                    let _ = writeln!(out, "[{status}] #{}: {}", item.id, item.text);
                }
                if state.items.iter().all(|i| i.done) {
                    out.push_str("All tasks completed. Use 'clear' to clean up.");
                }
                Ok(ToolResult {
                    output: out,
                    is_error: false,
                })
            }
            "clear" => {
                let count = state.items.len();
                state.items.clear();
                Ok(ToolResult {
                    output: format!("Cleared {count} tasks."),
                    is_error: false,
                })
            }
            _ => Ok(ToolResult {
                output: format!("Unknown action: {action}"),
                is_error: true,
            }),
        }
    }
}
