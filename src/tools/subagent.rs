use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use chrono::Local;
use regex::Regex;
use serde_json::json;

use super::execute_python_script::ExecutePythonScriptTool;
use super::shell::ShellTool;
use super::{Tool, ToolResult};
use crate::providers::{
    ChatOptions, ContentPart, ConversationMessage, Provider, Role, ToolCall, ToolSpec,
};
use crate::util::text::TruncateForUi;

const MAX_ITERATIONS: usize = 10;
const MAX_TOOL_OUTPUT_CHARS: usize = 10_000_000;
const MAX_TOKENS: u32 = 4096;
const EXECUTOR_DIR: &str = "~/.zerda/executor_jobs";
const MAX_KEY_OUTPUT_CHARS: usize = 6000;
const EXECUTOR_SYSTEM_PROMPT: &str = include_str!("../prompts/executor_system.md");
const EXECUTOR_DELEGATE_TEMPLATE: &str = include_str!("../prompts/executor_delegate.md");
const PRIMITIVES_ROOT_ENV: &str = "ZERDA_PRIMITIVES_ROOT";
const PRIMITIVES_ROOT: &str = "code_primitives/python";
const PRIMITIVES_CATALOG_FILE: &str = "primitives_catalog.md";
const TELEMETRY_FILE: &str = "telemetry.jsonl";
const FIRECRAWL_KEY_ENV: &str = "FIRECRAWL_API_KEY";
const FIRECRAWL_ALT_KEY_ENV: &str = "FIRECRAWL_KEY";
const DEFAULT_SYSTEM_PRIMITIVES_ROOT: &str = "/usr/local/share/zerda/code_primitives/python";
const MAX_PRIMITIVES_IN_PROMPT: usize = 3;
const MAX_SIGNATURE_CHARS: usize = 180;
const MAX_SUMMARY_CHARS: usize = 96;
const MAX_CONTRACT_CHARS: usize = 240;

#[derive(Clone)]
struct PrimitiveRuntime {
    catalog_slim: String,
    primitives_py_root: Option<PathBuf>,
    bootstrap_path: Option<PathBuf>,
    firecrawl_enabled: bool,
}

#[derive(Clone)]
struct PrimitiveFunction {
    name: String,
    signature: String,
    summary: String,
    output_contract: String,
}

pub struct SubAgentTool {
    provider: Arc<dyn Provider>,
    chat_opts: ChatOptions,
    tool_timeout: u64,
}

impl SubAgentTool {
    pub fn new(provider: Arc<dyn Provider>, mut chat_opts: ChatOptions, tool_timeout: u64) -> Self {
        chat_opts.max_tokens = Some(MAX_TOKENS);
        Self {
            provider,
            chat_opts,
            tool_timeout,
        }
    }
}

#[async_trait]
impl Tool for SubAgentTool {
    fn name(&self) -> &str {
        "delegate_to_executor"
    }

    fn description(&self) -> &str {
        "Delegate mechanical execution to the executor. Use a compact structured brief and let the executor \
         generate and run an async Python script under ./.zerda/executor_jobs/. \
         Return only key results and artifact paths. \
         Prefer goal-oriented delegation and avoid binding implementation details unless user-required."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "brief": {
                    "type": "string",
                    "description": "Goal-oriented delegation brief. Required fields: GOAL, INPUT, CONSTRAINTS, DONE_WHEN, RETURN. \
                                    CONSTRAINTS must describe WHAT to achieve, never HOW. \
                                    Bad: 'Use requests to fetch HTML, parse with BeautifulSoup'. \
                                    Good: 'Extract main article text; ignore navigation and ads'."
                },
                "task_name": {
                    "type": "string",
                    "description": "Optional short name used in executor artifact file names"
                }
            },
            "required": ["brief"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> Result<ToolResult> {
        let brief = args
            .get("brief")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required parameter: brief"))?;
        let task_name = args.get("task_name").and_then(|v| v.as_str()).unwrap_or("");
        let artifact = prepare_executor_artifacts(task_name, brief)?;
        let primitives = load_primitives_runtime(&artifact, brief)?;
        let user_message = build_executor_user_message(brief, &artifact);
        let system_prompt = build_executor_system_prompt(&primitives);

        let inner_tools: Vec<Box<dyn Tool>> = vec![
            Box::new(ExecutePythonScriptTool::new(
                artifact.script_path.clone(),
                artifact.log_path.clone(),
                artifact.out_path.clone(),
                artifact.telemetry_path.clone(),
                self.tool_timeout,
                primitives.primitives_py_root.clone(),
                primitives.bootstrap_path.clone(),
                primitives.firecrawl_enabled,
            )),
            Box::new(ShellTool::new(self.tool_timeout)),
        ];
        let tool_specs: Vec<ToolSpec> = inner_tools.iter().map(|t| t.spec()).collect();
        let tool_map: HashMap<&str, &dyn Tool> =
            inner_tools.iter().map(|t| (t.name(), t.as_ref())).collect();

        let system = ConversationMessage::system(system_prompt);

        let mut history = vec![system, ConversationMessage::user(user_message)];
        let mut last_text = String::new();

        for _ in 0..MAX_ITERATIONS {
            let response = self
                .provider
                .chat(&history, &tool_specs, &self.chat_opts)
                .await?;
            if let Some(text) = &response.text {
                if !text.trim().is_empty() {
                    last_text = text.clone();
                }
            }

            if response.tool_calls.is_empty() {
                let failed = executor_result_failed(&artifact);
                let output = build_executor_result(
                    response.text.as_deref().unwrap_or(""),
                    &artifact,
                    failed,
                );
                return Ok(ToolResult {
                    output,
                    is_error: failed,
                });
            }

            let mut assistant_msg = ConversationMessage {
                role: Role::Assistant,
                content: Vec::new(),
                tool_calls: response.tool_calls.clone(),
                reasoning_content: response.reasoning_content.clone(),
                thinking_blocks: response.thinking_blocks.clone(),
            };
            if let Some(ref text) = response.text {
                if !text.is_empty() {
                    assistant_msg.content.push(ContentPart::Text(text.clone()));
                }
            }
            history.push(assistant_msg);

            for tc in &response.tool_calls {
                let result = execute_inner_tool(&tool_map, tc).await;
                history.push(ConversationMessage::tool_result(
                    &tc.id,
                    result.output.truncate_for_ui(MAX_TOOL_OUTPUT_CHARS),
                    result.is_error,
                ));
            }
        }

        let final_response = self.provider.chat(&history, &[], &self.chat_opts).await?;
        let final_text = final_response
            .text
            .clone()
            .filter(|t| !t.trim().is_empty())
            .unwrap_or(last_text);
        let failed = executor_result_failed(&artifact);
        Ok(ToolResult {
            output: build_executor_result(final_text.as_str(), &artifact, failed),
            is_error: failed,
        })
    }
}

async fn execute_inner_tool(tool_map: &HashMap<&str, &dyn Tool>, call: &ToolCall) -> ToolResult {
    if let Some(tool) = tool_map.get(call.name.as_str()) {
        match tool.execute(call.arguments.clone()).await {
            Ok(result) => result,
            Err(e) => ToolResult {
                output: format!("Tool execution error: {e}"),
                is_error: true,
            },
        }
    } else {
        ToolResult {
            output: format!("Unknown tool: {}", call.name),
            is_error: true,
        }
    }
}

struct ExecutorArtifacts {
    script_path: PathBuf,
    log_path: PathBuf,
    out_path: PathBuf,
    meta_path: PathBuf,
    telemetry_path: PathBuf,
    catalog_path: PathBuf,
}

fn prepare_executor_artifacts(task_name: &str, brief: &str) -> Result<ExecutorArtifacts> {
    let root = crate::config::resolve_path(EXECUTOR_DIR);
    std::fs::create_dir_all(&root)?;

    let now = Local::now();
    let day = now.format("%Y%m%d").to_string();
    let time = now.format("%H%M%S").to_string();
    let basis = if task_name.trim().is_empty() {
        brief
    } else {
        task_name
    };
    let slug = sanitize_slug(basis);
    let task_dir = root.join(day).join(format!("{time}_{slug}"));
    std::fs::create_dir_all(&task_dir)?;

    let script_path = task_dir.join("script.py");
    let log_path = task_dir.join("run.log");
    let out_path = task_dir.join("result.out");
    let meta_path = task_dir.join("task.meta");
    let telemetry_path = task_dir.join(TELEMETRY_FILE);
    let catalog_path = task_dir.join(PRIMITIVES_CATALOG_FILE);

    let meta = format!(
        "created_at: {}\nscript: {}\nlog: {}\nout: {}\ntelemetry: {}\nprimitives_catalog: {}\nbrief:\n{}\n",
        Local::now().format("%Y-%m-%d %H:%M:%S"),
        script_path.display(),
        log_path.display(),
        out_path.display(),
        telemetry_path.display(),
        catalog_path.display(),
        brief
    );
    std::fs::write(&meta_path, meta)?;

    Ok(ExecutorArtifacts {
        script_path,
        log_path,
        out_path,
        meta_path,
        telemetry_path,
        catalog_path,
    })
}

fn build_executor_user_message(brief: &str, a: &ExecutorArtifacts) -> String {
    EXECUTOR_DELEGATE_TEMPLATE
        .replace("{{BRIEF}}", brief)
        .replace("{{SCRIPT_PATH}}", &a.script_path.display().to_string())
        .replace("{{LOG_PATH}}", &a.log_path.display().to_string())
        .replace("{{OUT_PATH}}", &a.out_path.display().to_string())
}

fn build_executor_system_prompt(primitives: &PrimitiveRuntime) -> String {
    EXECUTOR_SYSTEM_PROMPT
        .replace("{{PRIMITIVES_CATALOG}}", &primitives.catalog_slim)
        .trim_end()
        .to_string()
}

fn executor_result_failed(a: &ExecutorArtifacts) -> bool {
    let content = std::fs::read_to_string(&a.out_path).unwrap_or_default();
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return true;
    }
    if trimmed.eq_ignore_ascii_case("timeout or error") {
        return true;
    }
    trimmed.contains("Traceback (most recent call last)")
}

fn build_executor_result(summary: &str, a: &ExecutorArtifacts, failed: bool) -> String {
    let out_content = std::fs::read_to_string(&a.out_path).unwrap_or_default();
    let primary = if !out_content.trim().is_empty() {
        out_content.truncate_for_ui(MAX_KEY_OUTPUT_CHARS)
    } else if !summary.trim().is_empty() {
        summary.trim().to_string()
    } else {
        "(executor returned empty content)".to_string()
    };
    let status_line = if failed {
        "[executor_status: partial]"
    } else {
        "[executor_status: ok]"
    };
    format!(
        "{status_line}\n{primary}\n\n[artifacts]\nscript: {script}\nresult: {out}\nlog: {log}\nmeta: {meta}\ntelemetry: {telemetry}\nprimitives_catalog: {catalog}",
        script = a.script_path.display(),
        out = a.out_path.display(),
        log = a.log_path.display(),
        meta = a.meta_path.display(),
        telemetry = a.telemetry_path.display(),
        catalog = a.catalog_path.display(),
    )
}

fn sanitize_slug(raw: &str) -> String {
    let mut out = String::new();
    let mut prev_underscore = false;
    for ch in raw.chars().flat_map(char::to_lowercase) {
        let valid = ch.is_ascii_alphanumeric();
        if valid {
            out.push(ch);
            prev_underscore = false;
        } else if !prev_underscore {
            out.push('_');
            prev_underscore = true;
        }
        if out.len() >= 48 {
            break;
        }
    }
    let trimmed = out.trim_matches('_');
    if trimmed.is_empty() {
        "task".to_string()
    } else {
        trimmed.to_string()
    }
}

fn load_primitives_runtime(artifact: &ExecutorArtifacts, brief: &str) -> Result<PrimitiveRuntime> {
    let primitives_py_root = resolve_primitives_root()?;
    let primitives_dir = primitives_py_root.join("primitives");
    let bootstrap_path = primitives_py_root.join("bootstrap.py");
    let has_firecrawl_key = read_non_empty_env(FIRECRAWL_KEY_ENV)
        .or_else(|| read_non_empty_env(FIRECRAWL_ALT_KEY_ENV))
        .is_some();
    let has_runtime_files = primitives_dir.exists() && bootstrap_path.exists();
    let firecrawl_enabled = has_firecrawl_key && has_runtime_files;

    let (catalog_slim, catalog_full) = if !has_runtime_files {
        let msg = "### Available Code Primitives\n- unavailable: code_primitives runtime files are missing".to_string();
        (msg.clone(), msg)
    } else {
        let functions = discover_primitive_functions(&primitives_dir, has_firecrawl_key)?;
        let selected = select_primitives_for_brief(&functions, brief);
        if functions.is_empty() && has_firecrawl_key {
            let msg = "### Available Code Primitives\n- unavailable: no primitive functions discovered".to_string();
            (msg.clone(), msg)
        } else if functions.is_empty() {
            let msg = format!(
                "### Available Code Primitives\n- no active primitives found without {} (or {})",
                FIRECRAWL_KEY_ENV, FIRECRAWL_ALT_KEY_ENV
            );
            (msg.clone(), msg)
        } else {
            (
                render_catalog_slim(&selected, has_firecrawl_key),
                render_catalog_full(&selected, has_firecrawl_key),
            )
        }
    };

    fs::write(&artifact.catalog_path, &catalog_full)?;

    Ok(PrimitiveRuntime {
        catalog_slim,
        primitives_py_root: primitives_py_root.exists().then_some(primitives_py_root),
        bootstrap_path: bootstrap_path.exists().then_some(bootstrap_path),
        firecrawl_enabled,
    })
}

fn discover_primitive_functions(
    dir: &PathBuf,
    has_firecrawl_key: bool,
) -> Result<Vec<PrimitiveFunction>> {
    let mut files = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if path.extension().and_then(|v| v.to_str()) != Some("py") {
            continue;
        }
        let name = path
            .file_name()
            .and_then(|v| v.to_str())
            .unwrap_or_default();
        if matches!(name, "__init__.py" | "types.py" | "base.py" | "catalog.py") {
            continue;
        }
        files.push(path);
    }
    files.sort();

    let mut result = Vec::new();
    for path in files {
        let content = fs::read_to_string(&path)?;
        result.extend(parse_primitive_file(&content, has_firecrawl_key));
    }
    Ok(result)
}

fn parse_primitive_file(content: &str, has_firecrawl_key: bool) -> Vec<PrimitiveFunction> {
    let def_re = Regex::new(
        r"(?m)^async\s+def\s+([A-Za-z_][A-Za-z0-9_]*)\s*\(([^)]*)\)\s*(?:->\s*([^:]+))?:",
    )
    .expect("valid regex");
    let mut out = Vec::new();

    for cap in def_re.captures_iter(content) {
        let whole = if let Some(m) = cap.get(0) {
            m
        } else {
            continue;
        };
        let name = cap
            .get(1)
            .map(|m| m.as_str().to_string())
            .unwrap_or_default();
        if name.starts_with('_') {
            continue;
        }
        if name.starts_with("firecrawl_") && !has_firecrawl_key {
            continue;
        }
        let args = cap
            .get(2)
            .map(|m| m.as_str().trim().to_string())
            .unwrap_or_default();
        let ret = cap
            .get(3)
            .map(|m| m.as_str().trim().to_string())
            .unwrap_or_default();
        let signature = if ret.is_empty() {
            format!("{name}({args})")
        } else {
            format!("{name}({args}) -> {ret}")
        };
        let doc = extract_docstring(content, whole.end());
        let summary = extract_section_line(&doc, "[What it does]")
            .or_else(|| first_non_empty_line(&doc))
            .unwrap_or_default();
        let output_contract =
            extract_contract_paths(&doc, "[Output Contract]").unwrap_or_default();
        out.push(PrimitiveFunction {
            name,
            signature,
            summary,
            output_contract,
        });
    }
    out
}

fn extract_docstring(content: &str, from: usize) -> String {
    let remaining = &content[from..];
    let trimmed = remaining.trim_start();
    let delimiter = if trimmed.starts_with("\"\"\"") {
        "\"\"\""
    } else if trimmed.starts_with("'''") {
        "'''"
    } else {
        return String::new();
    };

    let after_open = &trimmed[3..];
    if let Some(idx) = after_open.find(delimiter) {
        after_open[..idx].trim().to_string()
    } else {
        String::new()
    }
}

fn first_non_empty_line(text: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(ToString::to_string)
}

fn extract_section_line(text: &str, section: &str) -> Option<String> {
    let mut lines = text.lines().map(str::trim);
    while let Some(line) = lines.next() {
        if line.eq_ignore_ascii_case(section) {
            return lines
                .find(|candidate| !candidate.is_empty())
                .map(ToString::to_string);
        }
    }
    None
}

fn extract_contract_paths(text: &str, section: &str) -> Option<String> {
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let mut i = 0;
    while i < lines.len() {
        if lines[i].eq_ignore_ascii_case(section) {
            i += 1;
            let mut paths = Vec::new();
            while i < lines.len() {
                let line = lines[i];
                if line.starts_with('[') && line.ends_with(']') {
                    break;
                }
                if line.contains("res[\"") {
                    let entry = if let Some(idx) = line.find(" #") {
                        let code = line[..idx].trim();
                        let comment = line[idx + 2..].trim();
                        format!("{code} ({comment})")
                    } else {
                        line.to_string()
                    };
                    paths.push(entry);
                }
                i += 1;
            }
            if paths.is_empty() {
                return None;
            }
            return Some(paths.join("; "));
        }
        i += 1;
    }
    None
}

fn render_catalog_slim(functions: &[PrimitiveFunction], has_firecrawl_key: bool) -> String {
    let mut out = String::from("### Available Code Primitives\n");
    if !has_firecrawl_key {
        out.push_str(&format!(
            "- note: firecrawl primitives disabled, missing {} (or {})\n\n",
            FIRECRAWL_KEY_ENV, FIRECRAWL_ALT_KEY_ENV
        ));
    }
    for f in functions {
        out.push_str(&format!("- {}", f.name));
        if !f.summary.is_empty() {
            out.push_str(&format!(": {}", compact_inline(&f.summary, MAX_SUMMARY_CHARS)));
        }
        out.push('\n');
        if !f.output_contract.is_empty() {
            out.push_str(&format!(
                "  contract: {}\n",
                compact_inline(&f.output_contract, MAX_CONTRACT_CHARS)
            ));
        }
    }
    out
}

fn render_catalog_full(functions: &[PrimitiveFunction], has_firecrawl_key: bool) -> String {
    let mut out = String::from("### Available Code Primitives\n");
    if !has_firecrawl_key {
        out.push_str(&format!(
            "- note: firecrawl primitives disabled, missing {} (or {})\n\n",
            FIRECRAWL_KEY_ENV, FIRECRAWL_ALT_KEY_ENV
        ));
    }
    for f in functions {
        out.push_str(&format!(
            "- `{}`\n",
            compact_inline(&f.signature, MAX_SIGNATURE_CHARS)
        ));
        if !f.summary.is_empty() {
            out.push_str(&format!(
                "  use: {}\n",
                compact_inline(&f.summary, MAX_SUMMARY_CHARS)
            ));
        }
        if !f.output_contract.is_empty() {
            out.push_str(&format!(
                "  output_contract: {}\n",
                compact_inline(&f.output_contract, MAX_CONTRACT_CHARS)
            ));
        }
        out.push('\n');
    }
    out
}

fn read_non_empty_env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn resolve_primitives_root() -> Result<PathBuf> {
    if let Some(path) = read_non_empty_env(PRIMITIVES_ROOT_ENV) {
        return Ok(crate::config::resolve_path(&path));
    }

    let cwd = std::env::current_dir()?;
    let local_root = cwd.join(PRIMITIVES_ROOT);
    if local_root.exists() {
        return Ok(local_root);
    }

    Ok(PathBuf::from(DEFAULT_SYSTEM_PRIMITIVES_ROOT))
}

fn select_primitives_for_brief(
    functions: &[PrimitiveFunction],
    brief: &str,
) -> Vec<PrimitiveFunction> {
    if functions.len() <= MAX_PRIMITIVES_IN_PROMPT {
        return functions.to_vec();
    }

    let lower = brief.to_lowercase();
    let has_url_intent = lower.contains("http")
        || lower.contains("url")
        || lower.contains("网页")
        || lower.contains("页面")
        || lower.contains("blog")
        || lower.contains("fetch")
        || lower.contains("抓取");
    let has_search_intent = lower.contains("search")
        || lower.contains("find")
        || lower.contains("查找")
        || lower.contains("搜索");
    let has_summary_intent = lower.contains("summary")
        || lower.contains("summarize")
        || lower.contains("总结")
        || lower.contains("正文")
        || lower.contains("main content");

    let mut scored: Vec<(i32, PrimitiveFunction)> = functions
        .iter()
        .cloned()
        .map(|f| {
            let mut score = 0;
            let name = f.name.as_str();
            if lower.contains(name) {
                score += 6;
            }
            if has_url_intent && name.contains("scrape") {
                score += 4;
            }
            if has_search_intent && name.contains("search") {
                score += 4;
            }
            if has_summary_intent && name.contains("extract_main_text") {
                score += 3;
            }
            if score == 0 {
                score = 1;
            }
            (score, f)
        })
        .collect();

    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.name.cmp(&b.1.name)));
    scored
        .into_iter()
        .take(MAX_PRIMITIVES_IN_PROMPT)
        .map(|(_, f)| f)
        .collect()
}

fn compact_inline(text: &str, max_chars: usize) -> String {
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.chars().count() <= max_chars {
        return normalized;
    }
    let mut out = String::new();
    for ch in normalized.chars() {
        if out.chars().count() >= max_chars {
            break;
        }
        out.push(ch);
    }
    format!("{out}...")
}
