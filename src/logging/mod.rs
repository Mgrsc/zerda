use serde_json::Value;
use std::sync::{LazyLock, RwLock};

#[derive(Debug, Clone)]
struct RuntimeLogOptions {
    debug_plaintext: bool,
    stream_progress_interval_ms: u64,
}

impl Default for RuntimeLogOptions {
    fn default() -> Self {
        Self {
            debug_plaintext: false,
            stream_progress_interval_ms: 2000,
        }
    }
}

static RUNTIME_LOG_OPTIONS: LazyLock<RwLock<RuntimeLogOptions>> =
    LazyLock::new(|| RwLock::new(RuntimeLogOptions::default()));

pub fn set_runtime_log_options(debug_plaintext: bool, stream_progress_interval_ms: u64) {
    if let Ok(mut options) = RUNTIME_LOG_OPTIONS.write() {
        options.debug_plaintext = debug_plaintext;
        options.stream_progress_interval_ms = stream_progress_interval_ms.max(200);
    }
}

pub fn debug_plaintext_enabled() -> bool {
    RUNTIME_LOG_OPTIONS
        .read()
        .map(|v| v.debug_plaintext)
        .unwrap_or(false)
}

pub fn stream_progress_interval_ms() -> u64 {
    RUNTIME_LOG_OPTIONS
        .read()
        .map(|v| v.stream_progress_interval_ms)
        .unwrap_or(2000)
}

pub fn summarize_http_body(text: &str) -> String {
    if debug_plaintext_enabled() {
        text.replace('\r', "\\r")
    } else {
        summarize_text_with_preview(text, 220)
    }
}

pub fn text_fingerprint(text: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in text.as_bytes() {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{hash:016x}")
}

pub fn summarize_text(text: &str) -> String {
    format!("len={},fp={}", text.chars().count(), text_fingerprint(text))
}

pub fn summarize_text_with_preview(text: &str, max_chars: usize) -> String {
    let total = text.chars().count();
    let preview = text
        .chars()
        .take(max_chars)
        .collect::<String>()
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t");
    let suffix = if total > max_chars { "..." } else { "" };
    format!(
        "len={},fp={},preview=\"{}{}\"",
        total,
        text_fingerprint(text),
        preview,
        suffix
    )
}

pub fn summarize_json(value: &Value) -> String {
    match value {
        Value::Object(map) => {
            let mut fields = Vec::new();
            for (k, v) in map.iter().take(8) {
                fields.push(format!("{k}:{}", value_shape(v)));
            }
            format!(
                "object(keys={},fields=[{}],bytes={})",
                map.len(),
                fields.join(","),
                value.to_string().len()
            )
        }
        Value::Array(arr) => {
            let sample = arr
                .iter()
                .take(5)
                .map(value_shape)
                .collect::<Vec<_>>()
                .join(",");
            format!(
                "array(len={},items=[{}],bytes={})",
                arr.len(),
                sample,
                value.to_string().len()
            )
        }
        _ => value_shape(value),
    }
}

fn value_shape(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(_) => "bool".to_string(),
        Value::Number(_) => "number".to_string(),
        Value::String(s) => format!("string({})", s.chars().count()),
        Value::Array(arr) => format!("array({})", arr.len()),
        Value::Object(map) => format!("object({})", map.len()),
    }
}
