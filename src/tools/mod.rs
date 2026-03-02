use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use tokio::sync::RwLock;

use crate::providers::ToolSpec;
use crate::reflection::ReflectionEngine;
use crate::skills::Skill;
use crate::tts::TtsProvider;

pub mod execute_python_script;
pub mod reload;
pub mod schema_compat;
pub mod search_docs;
pub mod shell;
pub mod skill;
pub mod subagent;
pub mod todo;
pub mod tts;

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
    pub config_path: Option<PathBuf>,
    pub reload_signal: reload::ReloadSignal,
    pub disabled_primitives: Vec<String>,
}

pub struct BuiltinToolsDependencies {
    pub tts_provider: Option<Arc<dyn TtsProvider>>,
    pub skills: Arc<RwLock<Vec<Skill>>>,
    pub skill_cache: Arc<RwLock<HashMap<String, String>>>,
    pub subagent_provider: Option<(Arc<dyn Provider>, ChatOptions)>,
    pub reflection: Option<Arc<ReflectionEngine>>,
}

pub struct BuiltinToolsContext {
    pub tool_timeout: u64,
    pub config_path: Option<PathBuf>,
    pub reload_signal: reload::ReloadSignal,
    pub disabled_primitives: Vec<String>,
    pub tts_provider: Option<Arc<dyn TtsProvider>>,
    pub skills: Arc<RwLock<Vec<Skill>>>,
    pub skill_cache: Arc<RwLock<HashMap<String, String>>>,
    pub subagent_provider: Option<(Arc<dyn Provider>, ChatOptions)>,
    pub reflection: Option<Arc<ReflectionEngine>>,
}

impl BuiltinToolsContext {
    pub fn new(runtime: BuiltinToolsRuntime, dependencies: BuiltinToolsDependencies) -> Self {
        Self {
            tool_timeout: runtime.tool_timeout,
            config_path: runtime.config_path,
            reload_signal: runtime.reload_signal,
            disabled_primitives: runtime.disabled_primitives,
            tts_provider: dependencies.tts_provider,
            skills: dependencies.skills,
            skill_cache: dependencies.skill_cache,
            subagent_provider: dependencies.subagent_provider,
            reflection: dependencies.reflection,
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
        config_path,
        reload_signal,
        disabled_primitives,
        tts_provider,
        skills,
        skill_cache,
        subagent_provider,
        reflection,
    } = ctx;

    let (todo_tool, todo_handle) = todo::TodoTool::new();
    let mut tools: Vec<Box<dyn Tool>> = vec![
        Box::new(reload::ReloadTool::new(config_path, reload_signal)),
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
            reflection,
            disabled_primitives,
        )));
    }
    if let Some(tool) = search_docs::SearchDocsTool::try_from_env() {
        tools.push(Box::new(tool));
    }
    (tools, todo_handle)
}
