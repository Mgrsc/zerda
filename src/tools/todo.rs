use std::collections::HashMap;
use std::fmt::Write as _;
use std::sync::{Arc, Mutex};

use anyhow::Result;
use async_trait::async_trait;
use serde_json::json;

use super::{Tool, ToolResult};

pub struct TodoItem {
    pub id: usize,
    pub text: String,
    pub done: bool,
}

pub struct TodoState {
    pub items: Vec<TodoItem>,
    pub next_id: usize,
}

impl TodoState {
    fn new() -> Self {
        Self {
            items: Vec::new(),
            next_id: 1,
        }
    }
}

pub struct TodoTool {
    store: Arc<Mutex<HashMap<String, TodoState>>>,
    active_session: Arc<Mutex<String>>,
}

impl TodoTool {
    pub fn new() -> (Self, TodoHandle) {
        let store = Arc::new(Mutex::new(HashMap::new()));
        let active_session = Arc::new(Mutex::new("cli".to_string()));
        let handle = TodoHandle {
            store: Arc::clone(&store),
            active_session: Arc::clone(&active_session),
        };
        (
            Self {
                store,
                active_session,
            },
            handle,
        )
    }

    fn current_session(&self) -> String {
        self.active_session.lock().unwrap().clone()
    }
}

#[async_trait]
impl Tool for TodoTool {
    fn name(&self) -> &str {
        "todo"
    }

    fn description(&self) -> &str {
        "IMPORTANT: You MUST proactively use this tool whenever the user's request \
         involves 2+ steps, multiple files, or any non-trivial work. Create tasks \
         BEFORE starting work to plan your approach, then mark each done as you go. \
         Failure to use this tool for complex requests leads to missed steps and \
         incomplete work. Supports add, edit, done, list, and clear."
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

        let session_key = self.current_session();
        let mut store = self.store.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
        let state = store.entry(session_key).or_insert_with(TodoState::new);

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

pub struct TodoHandle {
    store: Arc<Mutex<HashMap<String, TodoState>>>,
    active_session: Arc<Mutex<String>>,
}

impl TodoHandle {
    pub fn set_session(&self, id: &str) {
        *self.active_session.lock().unwrap() = id.to_string();
    }

    pub fn pending_count(&self) -> usize {
        let Ok(session) = self.active_session.lock() else {
            return 0;
        };
        let Ok(store) = self.store.lock() else {
            return 0;
        };
        store
            .get(session.as_str())
            .map(|s| s.items.iter().filter(|i| !i.done).count())
            .unwrap_or(0)
    }

    pub fn pending_reminder(&self) -> Option<String> {
        let session = self.active_session.lock().ok()?;
        let store = self.store.lock().ok()?;
        let state = store.get(session.as_str())?;
        let has_pending = state.items.iter().any(|i| !i.done);
        if !has_pending {
            return None;
        }
        let mut buf = String::from(
            "<system-reminder>\nYou have pending TODO items. You MUST work through all tasks and mark each one done upon completion.\n\n",
        );
        for item in &state.items {
            let status = if item.done { "x" } else { " " };
            let _ = writeln!(buf, "[{status}] #{}: {}", item.id, item.text);
        }
        buf.push_str("</system-reminder>");
        Some(buf)
    }
}
