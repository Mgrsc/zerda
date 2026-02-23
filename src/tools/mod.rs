use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use tokio::sync::RwLock;

use crate::memory::Memory;
use crate::providers::ToolSpec;
use crate::skills::Skill;
use crate::tts::TtsProvider;

pub mod mcp;
pub mod memory_tool;
pub mod read;
pub mod reload;
pub mod schema_compat;
pub mod shell;
pub mod skill;
pub mod subagent;
pub mod todo;
pub mod tts;
pub mod write;

#[derive(Debug, Clone)]
pub struct ToolResult {
    pub output: String,
    pub is_error: bool,
}

#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters_schema(&self) -> serde_json::Value;
    fn is_safe_for_concurrent(&self) -> bool {
        false
    }
    async fn execute(&self, args: serde_json::Value) -> Result<ToolResult>;

    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: self.name().to_string(),
            description: self.description().to_string(),
            parameters: schema_compat::sanitize_top_level_schema(
                self.parameters_schema(),
                self.name(),
            ),
        }
    }
}

use crate::providers::{ChatOptions, Provider};

pub struct BuiltinToolsRuntime {
    pub tool_timeout: u64,
    pub max_memory_chars: usize,
    pub config_path: Option<PathBuf>,
    pub reload_signal: reload::ReloadSignal,
}

pub struct BuiltinToolsDependencies {
    pub memory: Arc<Memory>,
    pub tts_provider: Option<Arc<dyn TtsProvider>>,
    pub skills: Arc<RwLock<Vec<Skill>>>,
    pub skill_cache: Arc<RwLock<HashMap<String, String>>>,
    pub subagent_provider: Option<(Arc<dyn Provider>, ChatOptions)>,
}

pub struct BuiltinToolsContext {
    pub tool_timeout: u64,
    pub memory: Arc<Memory>,
    pub max_memory_chars: usize,
    pub config_path: Option<PathBuf>,
    pub reload_signal: reload::ReloadSignal,
    pub tts_provider: Option<Arc<dyn TtsProvider>>,
    pub skills: Arc<RwLock<Vec<Skill>>>,
    pub skill_cache: Arc<RwLock<HashMap<String, String>>>,
    pub subagent_provider: Option<(Arc<dyn Provider>, ChatOptions)>,
}

impl BuiltinToolsContext {
    pub fn new(runtime: BuiltinToolsRuntime, dependencies: BuiltinToolsDependencies) -> Self {
        Self {
            tool_timeout: runtime.tool_timeout,
            memory: dependencies.memory,
            max_memory_chars: runtime.max_memory_chars,
            config_path: runtime.config_path,
            reload_signal: runtime.reload_signal,
            tts_provider: dependencies.tts_provider,
            skills: dependencies.skills,
            skill_cache: dependencies.skill_cache,
            subagent_provider: dependencies.subagent_provider,
        }
    }
}

impl From<(BuiltinToolsRuntime, BuiltinToolsDependencies)> for BuiltinToolsContext {
    fn from((runtime, dependencies): (BuiltinToolsRuntime, BuiltinToolsDependencies)) -> Self {
        Self::new(runtime, dependencies)
    }
}

pub fn builtin_tools(ctx: BuiltinToolsContext) -> (Vec<Box<dyn Tool>>, todo::TodoHandle) {
    let BuiltinToolsContext {
        tool_timeout,
        memory,
        max_memory_chars,
        config_path,
        reload_signal,
        tts_provider,
        skills,
        skill_cache,
        subagent_provider,
    } = ctx;

    let (todo_tool, todo_handle) = todo::TodoTool::new();
    let mut tools: Vec<Box<dyn Tool>> = vec![
        Box::new(shell::ShellTool::new(tool_timeout)),
        Box::new(read::ReadTool),
        Box::new(write::WriteTool),
        Box::new(reload::ReloadTool::new(config_path, reload_signal)),
        Box::new(memory_tool::MemoryTool::new(memory, max_memory_chars)),
        Box::new(skill::SkillTool::new(skills, skill_cache)),
        Box::new(todo_tool),
    ];
    if let Some(provider) = tts_provider {
        tools.push(Box::new(tts::TtsTool::new(provider)));
    }
    if let Some((provider, chat_opts)) = subagent_provider {
        tools.push(Box::new(subagent::SubAgentTool::new(
            provider,
            chat_opts,
            tool_timeout,
        )));
    }
    (tools, todo_handle)
}
