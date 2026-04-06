use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use anyhow::Result;
use chrono::{Local, Utc};
use serde::{Deserialize, Serialize};
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::channels::{ChannelMessage, ChannelMessageOrigin};
use crate::config;
use crate::providers::{ChatOptions, ConversationMessage, Provider};
use crate::ptc::parser::{PtcRequest, PtcRequestKind};

const PTC_JOB_DIR: &str = "~/.zerda/ptc_jobs";
const PRIMITIVES_ROOT_ENV: &str = "ZERDA_PRIMITIVES_ROOT";
const PRIMITIVES_ROOT: &str = "code_primitives/python";
const DEFAULT_SYSTEM_PRIMITIVES_ROOT: &str = "/usr/local/share/zerda/code_primitives/python";
const RESULT_COMPRESSION_TRIGGER_CHARS: usize = 8_000;
const MAX_RESULT_INLINE_CHARS: usize = 100_000;

#[derive(Debug, Clone)]
pub struct PtcSessionContext {
    pub channel: String,
    pub session_id: String,
    pub sender: String,
    pub main_model_request: String,
}

impl PtcSessionContext {
    pub fn session_key(&self) -> String {
        format!("{}:{}", self.channel, self.session_id)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PtcJobStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
    TimedOut,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PtcJobSummary {
    pub job_id: String,
    pub session_key: String,
    pub channel: String,
    pub session_id: String,
    pub sender: String,
    pub purpose: String,
    pub status: PtcJobStatus,
    pub created_at: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub elapsed_ms: Option<u128>,
    pub last_heartbeat_at: Option<String>,
    pub pid: Option<u32>,
    pub exit_code: Option<i32>,
    pub artifact_dir: PathBuf,
    pub script_path: PathBuf,
    pub out_path: PathBuf,
    pub log_path: PathBuf,
    pub telemetry_path: PathBuf,
    pub meta_path: PathBuf,
    pub status_path: PathBuf,
    pub request_context_path: PathBuf,
}

pub struct JobManager {
    inner: Arc<RwLock<std::collections::HashMap<String, PtcJobSummary>>>,
    tx: mpsc::Sender<ChannelMessage>,
    timeout_secs: u64,
    working_dir: PathBuf,
    primitives_py_roots: Vec<PathBuf>,
    bootstrap_path: Option<PathBuf>,
    disabled_primitives: Vec<String>,
    compression_provider: (Arc<dyn Provider>, ChatOptions),
}

impl JobManager {
    pub fn new(
        tx: mpsc::Sender<ChannelMessage>,
        timeout_secs: u64,
        disabled_primitives: Vec<String>,
        compression_provider: (Arc<dyn Provider>, ChatOptions),
    ) -> Self {
        let working_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let primitives_py_roots = resolve_primitives_roots(&working_dir);
        let bootstrap_path = primitives_py_roots.iter().find_map(|path| {
            let candidate = path.join("bootstrap.py");
            candidate.exists().then_some(candidate)
        });
        Self {
            inner: Arc::new(RwLock::new(std::collections::HashMap::new())),
            tx,
            timeout_secs,
            working_dir,
            primitives_py_roots,
            bootstrap_path,
            disabled_primitives,
            compression_provider,
        }
    }

    pub async fn launch_requests(
        &self,
        session: &PtcSessionContext,
        requests: Vec<PtcRequest>,
    ) -> Vec<PtcJobSummary> {
        let mut jobs = Vec::new();
        for request in requests {
            match self.launch_request(session, request).await {
                Ok(job) => jobs.push(job),
                Err(err) => {
                    let _ = self
                        .tx
                        .send(ChannelMessage {
                            sender: session.sender.clone(),
                            session_id: session.session_id.clone(),
                            content: format!(
                                "<PTC_RUNTIME_NOTICE source=\"runtime\" status=\"error\">Failed to start PTC job: {}</PTC_RUNTIME_NOTICE>",
                                xml_escape(&err.to_string())
                            ),
                            content_parts: None,
                            channel: session.channel.clone(),
                            origin: ChannelMessageOrigin::RuntimePtcNotice,
                            related_job_id: None,
                        })
                        .await;
                }
            }
        }
        jobs
    }

    pub async fn running_jobs_for_session(&self, session_key: &str) -> Vec<PtcJobSummary> {
        let mut jobs: Vec<PtcJobSummary> = self
            .inner
            .read()
            .await
            .values()
            .filter(|job| {
                job.session_key == session_key
                    && matches!(job.status, PtcJobStatus::Queued | PtcJobStatus::Running)
            })
            .cloned()
            .collect();
        jobs.sort_by(|a, b| a.created_at.cmp(&b.created_at));
        jobs
    }

    pub async fn list_jobs(&self, session_key: Option<&str>) -> Vec<PtcJobSummary> {
        let mut jobs: Vec<PtcJobSummary> = self
            .inner
            .read()
            .await
            .values()
            .filter(|job| session_key.is_none_or(|key| job.session_key == key))
            .cloned()
            .collect();
        jobs.sort_by(|a, b| a.created_at.cmp(&b.created_at));
        jobs
    }

    pub async fn get_job(&self, job_id: &str) -> Option<PtcJobSummary> {
        self.inner.read().await.get(job_id).cloned()
    }

    pub async fn cancel_job(&self, job_id: &str) -> Result<bool> {
        let mut guard = self.inner.write().await;
        let Some(job) = guard.get_mut(job_id) else {
            return Ok(false);
        };
        if !matches!(job.status, PtcJobStatus::Queued | PtcJobStatus::Running) {
            return Ok(false);
        }
        if let Some(pid) = job.pid {
            kill_pid(pid, "TERM").await?;
        }
        job.status = PtcJobStatus::Cancelled;
        job.finished_at = Some(now_iso());
        if let Some(started_at) = &job.started_at {
            job.elapsed_ms = elapsed_ms_from(started_at);
        }
        persist_status(job)?;
        Ok(true)
    }

    pub fn render_runtime_state_block(&self, jobs: &[PtcJobSummary]) -> Option<String> {
        if jobs.is_empty() {
            return None;
        }
        let mut out = String::from("<PTC_RUNTIME_STATE>\n");
        for job in jobs {
            let elapsed_ms = current_elapsed_ms(job);
            out.push_str(&format!(
                "  <RUNNING_JOB id=\"{}\" status=\"{}\" elapsed_ms=\"{}\" artifact_dir=\"{}\" status_path=\"{}\" out_path=\"{}\" log_path=\"{}\" telemetry_path=\"{}\">",
                xml_escape(&job.job_id),
                job_status_name(&job.status),
                elapsed_ms,
                xml_escape(&job.artifact_dir.display().to_string()),
                xml_escape(&job.status_path.display().to_string()),
                xml_escape(&job.out_path.display().to_string()),
                xml_escape(&job.log_path.display().to_string()),
                xml_escape(&job.telemetry_path.display().to_string())
            ));
            if job.purpose.is_empty() {
                out.push_str("background task");
            } else {
                out.push_str(&xml_escape(&job.purpose));
            }
            out.push_str("</RUNNING_JOB>\n");
        }
        out.push_str("</PTC_RUNTIME_STATE>");
        Some(out)
    }

    async fn launch_request(
        &self,
        session: &PtcSessionContext,
        request: PtcRequest,
    ) -> Result<PtcJobSummary> {
        let summary = prepare_job_summary(session, &request)?;
        match &request.kind {
            PtcRequestKind::Python { python } => {
                self.launch_python_request(session, summary, &request, python)
                    .await
            }
        }
    }

    async fn launch_python_request(
        &self,
        session: &PtcSessionContext,
        summary: PtcJobSummary,
        request: &PtcRequest,
        python: &str,
    ) -> Result<PtcJobSummary> {
        std::fs::create_dir_all(&summary.artifact_dir)?;
        write_request_context(&summary, &session.main_model_request)?;
        write_job_meta(&summary, request)?;
        std::fs::write(
            &summary.script_path,
            build_bootstrapped_code(
                python,
                self.bootstrap_path.as_ref(),
                &summary.out_path,
                &summary.log_path,
                &summary.telemetry_path,
            ),
        )?;

        let stdout_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&summary.log_path)?;
        let stderr_file = stdout_file.try_clone()?;

        let mut command = Command::new("python3");
        command
            .arg(&summary.script_path)
            .stdout(Stdio::from(stdout_file))
            .stderr(Stdio::from(stderr_file))
            .kill_on_drop(false)
            .current_dir(&self.working_dir)
            .env("PTC_OUT_PATH", &summary.out_path)
            .env("PTC_LOG_PATH", &summary.log_path)
            .env("PTC_TELEMETRY_PATH", &summary.telemetry_path)
            .env("PTC_ARTIFACT_DIR", &summary.artifact_dir)
            .env("PTC_WORKING_DIR", &self.working_dir)
            .env("PTC_JOB_ID", &summary.job_id)
            .env("PTC_SESSION_KEY", session.session_key())
            .env(
                "PTC_DISABLED_PRIMITIVES",
                serde_json::to_string(&self.disabled_primitives)
                    .unwrap_or_else(|_| "[]".to_string()),
            )
            .env(
                "PTC_PRIMITIVES_PY_ROOTS",
                serde_json::to_string(&self.primitives_py_roots)
                    .unwrap_or_else(|_| "[]".to_string()),
            );
        if let Some(root) = self.primitives_py_roots.first() {
            command.env("PTC_PRIMITIVES_PY_ROOT", root);
        }

        let mut child = command.spawn()?;
        let pid = child.id();
        let mut summary = summary;
        summary.status = PtcJobStatus::Running;
        summary.started_at = Some(now_iso());
        summary.last_heartbeat_at = summary.started_at.clone();
        summary.pid = pid;
        persist_status(&summary)?;
        self.inner
            .write()
            .await
            .insert(summary.job_id.clone(), summary.clone());

        let inner = Arc::clone(&self.inner);
        let tx = self.tx.clone();
        let timeout_secs = self.timeout_secs;
        let job_id = summary.job_id.clone();
        let compression_provider = (
            Arc::clone(&self.compression_provider.0),
            self.compression_provider.1.clone(),
        );
        tokio::spawn(async move {
            let started = SystemTime::now();
            let wait_result =
                tokio::time::timeout(Duration::from_secs(timeout_secs), child.wait()).await;
            let (status, exit_code, monitor_error) = match wait_result {
                Ok(Ok(exit_status)) => {
                    let code = exit_status.code();
                    if exit_status.success() {
                        (PtcJobStatus::Succeeded, code, None)
                    } else {
                        (PtcJobStatus::Failed, code, None)
                    }
                }
                Ok(Err(err)) => (PtcJobStatus::Failed, None, Some(err.to_string())),
                Err(_) => {
                    if let Some(pid) = pid {
                        let _ = kill_pid(pid, "KILL").await;
                    }
                    let _ = child.wait().await;
                    (PtcJobStatus::TimedOut, None, None)
                }
            };

            let mut maybe_summary = None;
            {
                let mut guard = inner.write().await;
                if let Some(job) = guard.get_mut(&job_id) {
                    if !matches!(job.status, PtcJobStatus::Cancelled) {
                        job.status = status;
                    }
                    job.exit_code = exit_code;
                    job.finished_at = Some(now_iso());
                    job.elapsed_ms = started.elapsed().ok().map(|value| value.as_millis());
                    job.last_heartbeat_at = job.finished_at.clone();
                    if let Some(error) = monitor_error {
                        let _ = std::fs::OpenOptions::new()
                            .create(true)
                            .append(true)
                            .open(&job.log_path)
                            .and_then(|mut file| {
                                use std::io::Write as _;
                                writeln!(file, "\n[PTC monitor error] {error}")
                            });
                    }
                    if persist_status(job).is_ok() {
                        maybe_summary = Some(job.clone());
                    }
                }
            }

            if let Some(job) = maybe_summary {
                let message = build_runtime_message(&compression_provider, &job).await;
                let _ = tx.send(message).await;
            }
        });

        Ok(summary)
    }
}

fn prepare_job_summary(session: &PtcSessionContext, request: &PtcRequest) -> Result<PtcJobSummary> {
    let now = Local::now();
    let slug = sanitize_slug(if request.purpose.is_empty() {
        "ptc_job"
    } else {
        &request.purpose
    });
    let job_id = format!("ptc_{}", Uuid::new_v4().simple());
    let root = config::resolve_path(PTC_JOB_DIR);
    let artifact_dir = root.join(now.format("%Y%m%d").to_string()).join(format!(
        "{}_{}_{}",
        now.format("%H%M%S"),
        slug,
        &job_id[4..16]
    ));
    Ok(PtcJobSummary {
        job_id: job_id.clone(),
        session_key: session.session_key(),
        channel: session.channel.clone(),
        session_id: session.session_id.clone(),
        sender: session.sender.clone(),
        purpose: request.purpose.clone(),
        status: PtcJobStatus::Queued,
        created_at: now_iso(),
        started_at: None,
        finished_at: None,
        elapsed_ms: None,
        last_heartbeat_at: None,
        pid: None,
        exit_code: None,
        script_path: artifact_dir.join("script.py"),
        out_path: artifact_dir.join("out.json"),
        log_path: artifact_dir.join("log.txt"),
        telemetry_path: artifact_dir.join("telemetry.jsonl"),
        meta_path: artifact_dir.join("meta.json"),
        status_path: artifact_dir.join("status.json"),
        request_context_path: artifact_dir.join("request.txt"),
        artifact_dir,
    })
}

fn write_job_meta(summary: &PtcJobSummary, request: &PtcRequest) -> Result<()> {
    let kind = match &request.kind {
        PtcRequestKind::Python { .. } => "python",
    };
    std::fs::write(
        &summary.meta_path,
        serde_json::to_string_pretty(&serde_json::json!({
            "job_id": summary.job_id,
            "session_key": summary.session_key,
            "purpose": request.purpose,
            "created_at": summary.created_at,
            "kind": kind,
            "request_context_path": summary.request_context_path,
        }))?,
    )?;
    Ok(())
}

fn write_request_context(summary: &PtcJobSummary, main_model_request: &str) -> Result<()> {
    std::fs::write(&summary.request_context_path, main_model_request)?;
    Ok(())
}

fn persist_status(job: &PtcJobSummary) -> Result<()> {
    std::fs::create_dir_all(&job.artifact_dir)?;
    std::fs::write(&job.status_path, serde_json::to_string_pretty(job)?)?;
    Ok(())
}

async fn build_runtime_message(
    compression_provider: &(Arc<dyn Provider>, ChatOptions),
    job: &PtcJobSummary,
) -> ChannelMessage {
    let result_content = std::fs::read_to_string(&job.out_path).unwrap_or_default();
    let inline_result = prepare_inline_result(compression_provider, job, &result_content).await;
    let status = match job.status {
        PtcJobStatus::Succeeded => "ok",
        PtcJobStatus::Failed => "error",
        PtcJobStatus::TimedOut => "timeout",
        PtcJobStatus::Cancelled => "cancelled",
        PtcJobStatus::Queued => "queued",
        PtcJobStatus::Running => "running",
    };
    let content = format!(
        "<PTC_TOOL_RESULT source=\"runtime\" job_id=\"{}\" status=\"{}\" elapsed_ms=\"{}\">\n\
<PTC_PURPOSE>{}</PTC_PURPOSE>\n\
<ARTIFACT_DIR>{}</ARTIFACT_DIR>\n\
<OUT_PATH>{}</OUT_PATH>\n\
<LOG_PATH>{}</LOG_PATH>\n\
<STATUS_PATH>{}</STATUS_PATH>\n\
<TELEMETRY_PATH>{}</TELEMETRY_PATH>\n\
<RESULT><![CDATA[\n{}\n]]></RESULT>\n\
</PTC_TOOL_RESULT>",
        job.job_id,
        status,
        current_elapsed_ms(job),
        xml_escape(&job.purpose),
        xml_escape(&job.artifact_dir.display().to_string()),
        xml_escape(&job.out_path.display().to_string()),
        xml_escape(&job.log_path.display().to_string()),
        xml_escape(&job.status_path.display().to_string()),
        xml_escape(&job.telemetry_path.display().to_string()),
        xml_cdata(&inline_result)
    );
    ChannelMessage {
        sender: job.sender.clone(),
        session_id: job.session_id.clone(),
        content,
        content_parts: None,
        channel: job.channel.clone(),
        origin: ChannelMessageOrigin::RuntimePtcResult,
        related_job_id: Some(job.job_id.clone()),
    }
}

async fn prepare_inline_result(
    compression_provider: &(Arc<dyn Provider>, ChatOptions),
    job: &PtcJobSummary,
    result_content: &str,
) -> String {
    let result_chars = result_content.chars().count();
    if result_chars <= RESULT_COMPRESSION_TRIGGER_CHARS {
        return result_content.to_string();
    }

    let request_context = std::fs::read_to_string(&job.request_context_path).unwrap_or_default();
    tracing::info!(
        job_id = %job.job_id,
        result_chars,
        request_context_chars = request_context.chars().count(),
        "Compressing oversized PTC result"
    );

    match compress_ptc_result(compression_provider, job, &request_context, result_content).await {
        Ok(compressed) => {
            let compressed_chars = compressed.chars().count();
            if compressed_chars > MAX_RESULT_INLINE_CHARS {
                tracing::warn!(
                    job_id = %job.job_id,
                    result_chars,
                    compressed_chars,
                    "Compressed PTC result still exceeded emergency inline limit"
                );
                return compression_failure_inline_result(
                    job,
                    result_chars,
                    "compressed_result_exceeded_inline_limit",
                    result_content,
                );
            }
            tracing::info!(
                job_id = %job.job_id,
                result_chars,
                compressed_chars,
                "Compressed oversized PTC result"
            );
            compressed
        }
        Err(err) => {
            tracing::warn!(
                job_id = %job.job_id,
                result_chars,
                error = %err,
                "Failed to compress oversized PTC result"
            );
            compression_failure_inline_result(
                job,
                result_chars,
                "compression_call_failed",
                result_content,
            )
        }
    }
}

async fn compress_ptc_result(
    compression_provider: &(Arc<dyn Provider>, ChatOptions),
    job: &PtcJobSummary,
    request_context: &str,
    result_content: &str,
) -> Result<String> {
    let prompt = build_result_compression_prompt(job, request_context, result_content);
    let messages = [ConversationMessage::user(prompt)];
    let response = compression_provider
        .0
        .chat(&messages, &compression_provider.1)
        .await?;
    let summary = response.text.unwrap_or_default().trim().to_string();
    if summary.is_empty() {
        anyhow::bail!("Empty compression summary");
    }
    Ok(summary)
}

fn build_result_compression_prompt(
    job: &PtcJobSummary,
    request_context: &str,
    result_content: &str,
) -> String {
    format!(
        "You are compressing a long detached PTC tool result for the main assistant.\n\
Preserve only the information needed for the assistant to continue the original task accurately.\n\
Keep critical facts, exact errors, identifiers, counts, paths, and conclusions.\n\
Drop redundancy, repeated keys, and verbose dumps.\n\
Output plain text only.\n\
Mention that the full raw payload remains available at the provided OUT_PATH.\n\n\
JOB_STATUS: {}\n\
PTC_PURPOSE: {}\n\
OUT_PATH: {}\n\n\
ORIGINAL_MAIN_MODEL_REQUEST:\n\
{}\n\n\
RAW_PTC_RESULT:\n\
{}",
        job_status_name(&job.status),
        job.purpose,
        job.out_path.display(),
        request_context,
        result_content
    )
}

fn compression_failure_inline_result(
    job: &PtcJobSummary,
    result_chars: usize,
    reason: &str,
    result_content: &str,
) -> String {
    if result_chars <= MAX_RESULT_INLINE_CHARS {
        return result_content.to_string();
    }
    format!(
        "PTC result compression could not produce an inline payload.\n\
reason: {reason}\n\
job_status: {}\n\
original_result_chars: {result_chars}\n\
full_raw_payload: {}",
        job_status_name(&job.status),
        job.out_path.display()
    )
}

fn build_bootstrapped_code(
    user_code: &str,
    bootstrap_path: Option<&PathBuf>,
    out_path: &Path,
    log_path: &Path,
    telemetry_path: &Path,
) -> String {
    let mut lines = Vec::new();
    lines.push("import os".to_string());
    lines.push(format!(
        "os.environ.setdefault(\"PTC_OUT_PATH\", {})",
        to_py_string(&out_path.display().to_string())
    ));
    lines.push(format!(
        "os.environ.setdefault(\"PTC_LOG_PATH\", {})",
        to_py_string(&log_path.display().to_string())
    ));
    lines.push(format!(
        "os.environ.setdefault(\"PTC_TELEMETRY_PATH\", {})",
        to_py_string(&telemetry_path.display().to_string())
    ));
    if let Some(path) = bootstrap_path {
        lines.push(format!(
            "_BOOTSTRAP_PATH = {}",
            to_py_string(&path.display().to_string())
        ));
        lines.push("if os.path.exists(_BOOTSTRAP_PATH):".to_string());
        lines.push("    with open(_BOOTSTRAP_PATH, \"r\", encoding=\"utf-8\") as _bf:".to_string());
        lines.push("        _bootstrap_src = _bf.read()".to_string());
        lines.push(
            "    exec(compile(_bootstrap_src, _BOOTSTRAP_PATH, \"exec\"), globals(), globals())"
                .to_string(),
        );
    }
    lines.push(String::new());
    lines.push("import asyncio".to_string());
    lines.push(String::new());
    lines.push("async def __zerda_ptc_main__():".to_string());
    let body = indent_python_block(user_code, 1);
    if body.trim().is_empty() {
        lines.push("    return None".to_string());
    } else {
        lines.push(body);
        lines.push("    return locals().get(\"result\")".to_string());
    }
    lines.push(String::new());
    lines.push("__zerda_ptc_return = asyncio.run(__zerda_ptc_main__())".to_string());
    lines.push(
        "__zerda_existing_result = read_ptc_result() if \"read_ptc_result\" in globals() else None"
            .to_string(),
    );
    lines.push("if __zerda_existing_result is None:".to_string());
    lines.push("    write_ptc_result(__zerda_ptc_return)".to_string());
    lines.join("\n")
}

fn to_py_string(value: &str) -> String {
    serde_json::to_string(&value).unwrap_or_else(|_| "\"\"".to_string())
}

fn indent_python_block(text: &str, depth: usize) -> String {
    let prefix = "    ".repeat(depth);
    text.lines()
        .map(|line| {
            if line.trim().is_empty() {
                String::new()
            } else {
                format!("{prefix}{line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

async fn kill_pid(pid: u32, signal: &str) -> Result<()> {
    let status = Command::new("kill")
        .arg(format!("-{signal}"))
        .arg(pid.to_string())
        .status()
        .await?;
    if status.success() {
        Ok(())
    } else {
        anyhow::bail!("kill -{signal} {pid} failed with status {status}");
    }
}

fn sanitize_slug(raw: &str) -> String {
    let mut out = String::new();
    let mut prev_underscore = false;
    for ch in raw.chars().flat_map(char::to_lowercase) {
        if ch.is_ascii_alphanumeric() {
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

fn now_iso() -> String {
    Utc::now().to_rfc3339()
}

fn elapsed_ms_from(iso: &str) -> Option<u128> {
    let parsed = chrono::DateTime::parse_from_rfc3339(iso).ok()?;
    let now = Utc::now();
    let delta = now.signed_duration_since(parsed.with_timezone(&Utc));
    delta.num_milliseconds().try_into().ok()
}

fn current_elapsed_ms(job: &PtcJobSummary) -> u128 {
    job.elapsed_ms
        .or_else(|| job.started_at.as_deref().and_then(elapsed_ms_from))
        .unwrap_or(0)
}

fn job_status_name(status: &PtcJobStatus) -> &'static str {
    match status {
        PtcJobStatus::Queued => "queued",
        PtcJobStatus::Running => "running",
        PtcJobStatus::Succeeded => "succeeded",
        PtcJobStatus::Failed => "failed",
        PtcJobStatus::TimedOut => "timed_out",
        PtcJobStatus::Cancelled => "cancelled",
    }
}

fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn xml_cdata(text: &str) -> String {
    text.replace("]]>", "]]]]><![CDATA[>")
}

fn resolve_primitives_roots(working_dir: &Path) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(path) = std::env::var(PRIMITIVES_ROOT_ENV)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    {
        roots.push(config::resolve_path(&path));
    } else {
        let local_root = working_dir.join(PRIMITIVES_ROOT);
        if local_root.exists() {
            roots.push(local_root);
        } else {
            roots.push(PathBuf::from(DEFAULT_SYSTEM_PRIMITIVES_ROOT));
        }
    }

    let custom_root = working_dir.join("custom_primitives");
    if custom_root.exists() {
        roots.push(working_dir.to_path_buf());
    }

    roots
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::Mutex;

    use anyhow::Result;
    use async_trait::async_trait;

    use crate::providers::ProviderResponse;

    enum StubReply {
        Text(String),
        Error(String),
    }

    struct StubProvider {
        replies: Mutex<VecDeque<StubReply>>,
        prompts: Mutex<Vec<String>>,
    }

    impl StubProvider {
        fn new(replies: Vec<StubReply>) -> Self {
            Self {
                replies: Mutex::new(VecDeque::from(replies)),
                prompts: Mutex::new(Vec::new()),
            }
        }

        fn prompts(&self) -> Vec<String> {
            self.prompts.lock().expect("prompts lock").clone()
        }
    }

    #[async_trait]
    impl Provider for StubProvider {
        async fn chat(
            &self,
            messages: &[ConversationMessage],
            _opts: &ChatOptions,
        ) -> Result<ProviderResponse> {
            let prompt = messages
                .first()
                .map(ConversationMessage::text_content)
                .unwrap_or_default();
            self.prompts.lock().expect("prompts lock").push(prompt);
            let reply = self
                .replies
                .lock()
                .expect("replies lock")
                .pop_front()
                .expect("reply");
            match reply {
                StubReply::Text(text) => Ok(ProviderResponse {
                    text: Some(text),
                    usage: None,
                    reasoning_content: None,
                    thinking_blocks: Vec::new(),
                }),
                StubReply::Error(message) => Err(anyhow::anyhow!(message)),
            }
        }
    }

    fn test_chat_options() -> ChatOptions {
        ChatOptions {
            model: "fast-test".to_string(),
            temperature: None,
            top_p: None,
            max_tokens: None,
        }
    }

    fn test_job_summary(root: &Path) -> PtcJobSummary {
        PtcJobSummary {
            job_id: "ptc_test_job".to_string(),
            session_key: "cli:test".to_string(),
            channel: "cli".to_string(),
            session_id: "test".to_string(),
            sender: "user".to_string(),
            purpose: "summarize search result".to_string(),
            status: PtcJobStatus::Succeeded,
            created_at: now_iso(),
            started_at: None,
            finished_at: None,
            elapsed_ms: Some(12),
            last_heartbeat_at: None,
            pid: None,
            exit_code: Some(0),
            artifact_dir: root.to_path_buf(),
            script_path: root.join("script.py"),
            out_path: root.join("out.json"),
            log_path: root.join("log.txt"),
            telemetry_path: root.join("telemetry.jsonl"),
            meta_path: root.join("meta.json"),
            status_path: root.join("status.json"),
            request_context_path: root.join("request.txt"),
        }
    }

    fn make_temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("zerda-job-manager-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    #[tokio::test]
    async fn compresses_oversized_result_with_request_context() {
        let root = make_temp_dir();
        let job = test_job_summary(&root);
        let raw_result = "x".repeat(RESULT_COMPRESSION_TRIGGER_CHARS + 1);
        std::fs::write(
            &job.request_context_path,
            "User asked for the final extracted answer.",
        )
        .expect("write request context");
        let provider = Arc::new(StubProvider::new(vec![StubReply::Text(
            "compressed result".to_string(),
        )]));
        let compression_provider = (provider.clone() as Arc<dyn Provider>, test_chat_options());

        let inline_result = prepare_inline_result(&compression_provider, &job, &raw_result).await;

        assert_eq!(inline_result, "compressed result");
        let prompts = provider.prompts();
        assert_eq!(prompts.len(), 1);
        assert!(prompts[0].contains("User asked for the final extracted answer."));
        assert!(prompts[0].contains("RAW_PTC_RESULT"));
        std::fs::remove_dir_all(root).expect("cleanup temp dir");
    }

    #[tokio::test]
    async fn keeps_small_result_without_calling_compression() {
        let root = make_temp_dir();
        let job = test_job_summary(&root);
        let raw_result = "x".repeat(RESULT_COMPRESSION_TRIGGER_CHARS);
        let provider = Arc::new(StubProvider::new(vec![]));
        let compression_provider = (provider.clone() as Arc<dyn Provider>, test_chat_options());

        let inline_result = prepare_inline_result(&compression_provider, &job, &raw_result).await;

        assert_eq!(inline_result, raw_result);
        assert!(provider.prompts().is_empty());
        std::fs::remove_dir_all(root).expect("cleanup temp dir");
    }

    #[tokio::test]
    async fn falls_back_to_raw_result_when_compression_fails_within_emergency_cap() {
        let root = make_temp_dir();
        let job = test_job_summary(&root);
        let raw_result = "x".repeat(RESULT_COMPRESSION_TRIGGER_CHARS + 1);
        std::fs::write(&job.request_context_path, "Need exact answer").expect("write request");
        let provider = Arc::new(StubProvider::new(vec![StubReply::Error(
            "provider failed".to_string(),
        )]));
        let compression_provider = (provider as Arc<dyn Provider>, test_chat_options());

        let inline_result = prepare_inline_result(&compression_provider, &job, &raw_result).await;

        assert_eq!(inline_result, raw_result);
        std::fs::remove_dir_all(root).expect("cleanup temp dir");
    }

    #[tokio::test]
    async fn falls_back_to_pointer_notice_when_compression_fails_above_emergency_cap() {
        let root = make_temp_dir();
        let job = test_job_summary(&root);
        let raw_result = "x".repeat(MAX_RESULT_INLINE_CHARS + 1);
        std::fs::write(&job.request_context_path, "Need exact answer").expect("write request");
        let provider = Arc::new(StubProvider::new(vec![StubReply::Error(
            "provider failed".to_string(),
        )]));
        let compression_provider = (provider as Arc<dyn Provider>, test_chat_options());

        let inline_result = prepare_inline_result(&compression_provider, &job, &raw_result).await;

        assert!(inline_result.contains("full_raw_payload"));
        assert!(inline_result.contains(&job.out_path.display().to_string()));
        assert!(inline_result.len() < raw_result.len());
        std::fs::remove_dir_all(root).expect("cleanup temp dir");
    }
}
