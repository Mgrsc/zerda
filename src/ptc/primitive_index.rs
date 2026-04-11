use std::path::{Path, PathBuf};

use anyhow::Result;
use regex::Regex;
use serde::Serialize;

use crate::config;
use crate::ptc::custom_packages;

const PRIMITIVES_ROOT_ENV: &str = "ZERDA_PRIMITIVES_ROOT";
const PRIMITIVES_ROOT: &str = "code_primitives/python";
const DEFAULT_SYSTEM_PRIMITIVES_ROOT: &str = "/usr/local/share/zerda/code_primitives/python";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PrimitiveSource {
    Internal,
    Custom,
}

impl PrimitiveSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Internal => "internal",
            Self::Custom => "custom",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PrimitiveMetadata {
    pub name: String,
    pub source: PrimitiveSource,
    pub path: PathBuf,
    pub signature: String,
    pub summary: String,
    pub args: String,
    pub output_contract: String,
    pub when_not_to_use: String,
    pub common_mistakes: String,
    pub requires: Vec<String>,
    pub enabled: bool,
    pub disabled_reason: Option<String>,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct PrimitiveIndex {
    items: Vec<PrimitiveMetadata>,
}

impl PrimitiveIndex {
    pub fn load(disabled_primitives: &[String]) -> Result<Self> {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let disabled: std::collections::HashSet<&str> =
            disabled_primitives.iter().map(String::as_str).collect();
        let mut items = Vec::new();

        let internal_root = resolve_internal_primitives_root(&cwd);
        if internal_root.exists() {
            let registered = parse_registered_primitives(&internal_root.join("catalog.py"))?;
            collect_registered_files(
                &internal_root,
                false,
                &registered,
                &mut items,
                &disabled,
                PrimitiveSource::Internal,
            )?;
        }

        let custom_root = cwd.join("custom_primitives");
        if custom_root.exists() {
            for primitive in custom_packages::ready_runtime_primitives(&cwd, disabled_primitives)? {
                let name = primitive.name;
                items.push(PrimitiveMetadata {
                    name: name.clone(),
                    source: PrimitiveSource::Custom,
                    path: primitive.source_path,
                    signature: primitive.call_shape,
                    summary: primitive.summary,
                    args: primitive.args,
                    output_contract: primitive.returns,
                    when_not_to_use: primitive.when_not_to_use,
                    common_mistakes: primitive.common_mistakes,
                    requires: primitive.requirements,
                    enabled: true,
                    disabled_reason: None,
                    tags: build_tags(&name, PrimitiveSource::Custom),
                });
            }
        }

        items.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(Self { items })
    }

    pub fn available_prompt_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .items
            .iter()
            .filter(|item| item.enabled)
            .map(|item| public_prompt_name(&item.name))
            .collect();
        names.sort();
        names.dedup();
        names
    }
}

fn resolve_internal_primitives_root(working_dir: &Path) -> PathBuf {
    if let Some(path) = std::env::var(PRIMITIVES_ROOT_ENV)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    {
        return config::resolve_path(&path).join("primitives");
    }

    let local_root = working_dir.join(PRIMITIVES_ROOT);
    if local_root.exists() {
        return local_root.join("primitives");
    }

    PathBuf::from(DEFAULT_SYSTEM_PRIMITIVES_ROOT).join("primitives")
}

fn public_prompt_name(raw: &str) -> String {
    if raw == "agent_browser" || raw.starts_with("agent_browser_") {
        return "agent_browser".to_string();
    }
    raw.to_string()
}

fn is_primitive_file(path: &Path) -> bool {
    if path.extension().and_then(|value| value.to_str()) != Some("py") {
        return false;
    }
    !matches!(
        path.file_name().and_then(|value| value.to_str()),
        Some("__init__.py" | "base.py" | "catalog.py" | "types.py")
    )
}

fn collect_registered_files(
    root: &Path,
    recursive: bool,
    registered: &std::collections::HashSet<String>,
    items: &mut Vec<PrimitiveMetadata>,
    disabled: &std::collections::HashSet<&str>,
    source: PrimitiveSource,
) -> Result<()> {
    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            if recursive {
                collect_registered_files(&path, true, registered, items, disabled, source)?;
            }
            continue;
        }
        if !is_primitive_file(&path) {
            continue;
        }
        for metadata in parse_primitive_file(&path, source, disabled)? {
            if registered.contains(&metadata.name) {
                items.push(metadata);
            }
        }
    }
    Ok(())
}

fn parse_registered_primitives(path: &Path) -> Result<std::collections::HashSet<String>> {
    if !path.exists() {
        return Ok(std::collections::HashSet::new());
    }
    let text = std::fs::read_to_string(path)?;
    let re = Regex::new(r#""([A-Za-z_][A-Za-z0-9_]*)"\s*:"#)?;
    Ok(re
        .captures_iter(&text)
        .filter_map(|captures| captures.get(1).map(|m| m.as_str().to_string()))
        .collect())
}

fn parse_primitive_file(
    path: &Path,
    source: PrimitiveSource,
    disabled: &std::collections::HashSet<&str>,
) -> Result<Vec<PrimitiveMetadata>> {
    let text = std::fs::read_to_string(path)?;
    let signature_re =
        Regex::new(r"(?s)async def\s+([A-Za-z_][A-Za-z0-9_]*)\s*(\([^)]*\)\s*->\s*[^\n:]+):")?;
    let mut items = Vec::new();
    for captures in signature_re.captures_iter(&text) {
        let Some(name) = captures.get(1).map(|m| m.as_str().to_string()) else {
            continue;
        };
        let Some(signature) = captures
            .get(0)
            .map(|m| m.as_str().trim_end_matches(':').trim().to_string())
        else {
            continue;
        };
        let docstring = extract_docstring(&text, &name);
        let summary = extract_section(&docstring, "What it does")
            .or_else(|| first_non_empty_line(&docstring))
            .unwrap_or_else(|| name.clone());
        let args = extract_section(&docstring, "Args").unwrap_or_default();
        let output_contract = extract_section(&docstring, "Output Contract").unwrap_or_default();
        let when_not_to_use = extract_section(&docstring, "When NOT to use").unwrap_or_default();
        let common_mistakes = extract_section(&docstring, "Common Mistakes").unwrap_or_default();
        items.push(PrimitiveMetadata {
            name: name.clone(),
            source,
            path: path.to_path_buf(),
            signature,
            summary,
            args,
            output_contract,
            when_not_to_use,
            common_mistakes,
            requires: infer_requirements(&text),
            enabled: !disabled.contains(name.as_str()),
            disabled_reason: disabled
                .contains(name.as_str())
                .then(|| "disabled by agent.disabled_primitives".to_string()),
            tags: build_tags(&name, source),
        });
    }
    Ok(items)
}

fn extract_docstring(text: &str, name: &str) -> String {
    let pattern = format!(
        r#"(?s)async def\s+{}\s*\([^)]*\)\s*->\s*[^\n:]+:\s*("""|''')(.*?)\1"#,
        regex::escape(name)
    );
    let Ok(re) = Regex::new(&pattern) else {
        return String::new();
    };
    re.captures(text)
        .and_then(|captures| captures.get(2).map(|m| m.as_str().trim().to_string()))
        .unwrap_or_default()
}

fn extract_section(docstring: &str, title: &str) -> Option<String> {
    let marker = format!("[{title}]");
    let start = docstring.find(&marker)?;
    let tail = &docstring[start + marker.len()..];
    let next = tail.find("\n    [").or_else(|| tail.find("\n["));
    let section = match next {
        Some(idx) => &tail[..idx],
        None => tail,
    };
    let value = section
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    (!value.is_empty()).then_some(value)
}

fn first_non_empty_line(docstring: &str) -> Option<String> {
    docstring
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_string)
}

fn infer_requirements(text: &str) -> Vec<String> {
    let mut requires = Vec::new();
    if text.contains("FIRECRAWL_API_KEY") {
        requires.push("FIRECRAWL_API_KEY".to_string());
    }
    if text.contains("agent-browser") || text.contains("AGENT_BROWSER_EXECUTABLE_PATH") {
        requires.push("agent-browser".to_string());
    }
    requires
}

fn build_tags(name: &str, source: PrimitiveSource) -> Vec<String> {
    let mut tags = vec![source.as_str().to_string()];
    tags.extend(name.split('_').map(str::to_string));
    tags.sort();
    tags.dedup();
    tags
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_prompt_name_collapses_agent_browser_methods() {
        assert_eq!(
            public_prompt_name("agent_browser_connect_cdp"),
            "agent_browser"
        );
        assert_eq!(public_prompt_name("agent_browser"), "agent_browser");
        assert_eq!(public_prompt_name("fs_read"), "fs_read");
    }

    #[test]
    fn resolve_internal_primitives_root_prefers_env_override() {
        let override_root = std::env::temp_dir()
            .join(format!("zerda-primitive-index-{}", std::process::id()))
            .join("python-root");
        unsafe {
            std::env::set_var(PRIMITIVES_ROOT_ENV, &override_root);
        }
        let resolved = resolve_internal_primitives_root(Path::new("/does/not/matter"));
        unsafe {
            std::env::remove_var(PRIMITIVES_ROOT_ENV);
        }
        assert_eq!(resolved, override_root.join("primitives"));
    }
}
