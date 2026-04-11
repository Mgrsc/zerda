use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tokio::process::Command;

const CUSTOM_PRIMITIVES_DIR: &str = "custom_primitives";
const CUSTOM_PACKAGE_CACHE_DIR: &str = "~/.zerda/custom_primitives/packages";
const PTC_PYTHON_ENV: &str = "ZERDA_PTC_PYTHON";
const DEFAULT_PTC_PYTHON: &str = "/opt/zerda-python/bin/python";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CustomPackageSyncStatus {
    PendingSync,
    Ready,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CustomPackageStateFile {
    pub digest: String,
    pub status: CustomPackageSyncStatus,
    pub message: String,
    pub python_executable: PathBuf,
    pub synced_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomPackageExport {
    pub name: String,
    pub module: String,
    pub callable: String,
    pub source_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomPackage {
    pub project_name: String,
    pub project_dir: PathBuf,
    pub manifest_path: PathBuf,
    pub lock_path: Option<PathBuf>,
    pub digest: String,
    pub optional: bool,
    pub dependencies: Vec<String>,
    pub external_commands: Vec<String>,
    pub playwright_browsers: Vec<String>,
    pub exports: Vec<CustomPackageExport>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomPackageRuntimeStatus {
    status: CustomPackageSyncStatus,
    message: String,
    python_executable: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CustomRuntimePrimitive {
    pub name: String,
    pub module: String,
    pub callable: String,
    pub python_executable: PathBuf,
    pub source_path: PathBuf,
    pub summary: String,
    pub call_shape: String,
    pub args: String,
    pub returns: String,
    pub when_not_to_use: String,
    pub common_mistakes: String,
    pub requirements: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct CustomPackageSyncReport {
    pub package_name: String,
    pub status: CustomPackageSyncStatus,
    pub message: String,
}

impl CustomPackageRuntimeStatus {
    pub fn is_ready(&self) -> bool {
        matches!(self.status, CustomPackageSyncStatus::Ready)
    }

    pub fn status_name(&self) -> &'static str {
        match self.status {
            CustomPackageSyncStatus::PendingSync => "pending_sync",
            CustomPackageSyncStatus::Ready => "ready",
            CustomPackageSyncStatus::Failed => "failed",
        }
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn python_executable(&self) -> Option<PathBuf> {
        self.python_executable.clone()
    }
}

impl CustomPackage {
    pub fn cache_slug(&self) -> String {
        sanitize_slug(&self.project_name)
    }

    pub fn env_dir(&self) -> PathBuf {
        resolve_custom_package_cache_root().join(self.cache_slug())
    }

    pub fn env_python_path(&self) -> PathBuf {
        if cfg!(windows) {
            self.env_dir().join("Scripts").join("python.exe")
        } else {
            self.env_dir().join("bin").join("python")
        }
    }

    pub fn state_path(&self) -> PathBuf {
        self.env_dir().join("state.json")
    }
}

#[derive(Debug, Deserialize)]
struct PyProjectManifest {
    project: ProjectSection,
    tool: ToolSection,
}

#[derive(Debug, Deserialize)]
struct ProjectSection {
    name: String,
    #[serde(default)]
    dependencies: Vec<String>,
}

#[derive(Debug, Deserialize, Default)]
struct ToolSection {
    zerda: ZerdaSection,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
struct ZerdaSection {
    #[serde(default)]
    optional: bool,
    #[serde(default)]
    external_commands: Vec<String>,
    #[serde(default)]
    playwright_browsers: Vec<String>,
    #[serde(default)]
    exports: Vec<ExportSection>,
}

#[derive(Debug, Deserialize)]
struct ExportSection {
    name: String,
    module: String,
    callable: String,
}

pub fn scan_custom_packages(working_dir: &Path) -> Result<Vec<CustomPackage>> {
    let root = working_dir.join(CUSTOM_PRIMITIVES_DIR);
    if !root.exists() {
        return Ok(Vec::new());
    }

    let mut packages = Vec::new();
    for entry in fs::read_dir(&root)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let project_dir = entry.path();
        let manifest_path = project_dir.join("pyproject.toml");
        if !manifest_path.exists() {
            continue;
        }
        packages.push(load_custom_package(
            working_dir,
            &project_dir,
            &manifest_path,
        )?);
    }
    packages.sort_by(|a, b| a.project_name.cmp(&b.project_name));
    Ok(packages)
}

pub fn package_runtime_status(package: &CustomPackage) -> CustomPackageRuntimeStatus {
    let path = package.state_path();
    let Some(state) = read_state_file(&path).ok().flatten() else {
        return CustomPackageRuntimeStatus {
            status: CustomPackageSyncStatus::PendingSync,
            message: "package has not been synced".to_string(),
            python_executable: None,
        };
    };

    if state.digest != package.digest {
        return CustomPackageRuntimeStatus {
            status: CustomPackageSyncStatus::PendingSync,
            message: "package manifest changed since last sync".to_string(),
            python_executable: None,
        };
    }

    if !matches!(state.status, CustomPackageSyncStatus::Ready) {
        return CustomPackageRuntimeStatus {
            status: state.status,
            message: state.message,
            python_executable: None,
        };
    }

    if !state.python_executable.exists() {
        return CustomPackageRuntimeStatus {
            status: CustomPackageSyncStatus::PendingSync,
            message: "synced interpreter is missing".to_string(),
            python_executable: None,
        };
    }

    if let Err(error) = ensure_external_commands_available(&package.external_commands) {
        return CustomPackageRuntimeStatus {
            status: CustomPackageSyncStatus::Failed,
            message: error.to_string(),
            python_executable: None,
        };
    }

    CustomPackageRuntimeStatus {
        status: CustomPackageSyncStatus::Ready,
        message: state.message,
        python_executable: Some(state.python_executable),
    }
}

pub fn read_state_file(path: &Path) -> Result<Option<CustomPackageStateFile>> {
    if !path.exists() {
        return Ok(None);
    }
    Ok(Some(serde_json::from_str(&fs::read_to_string(path)?)?))
}

pub fn write_state_file(path: &Path, state: &CustomPackageStateFile) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_string_pretty(state)?)?;
    Ok(())
}

pub fn resolve_custom_package_cache_root() -> PathBuf {
    std::env::var("ZERDA_CUSTOM_PRIMITIVES_CACHE_DIR")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(|value| crate::config::resolve_path(&value).join("packages"))
        .unwrap_or_else(|| crate::config::resolve_path(CUSTOM_PACKAGE_CACHE_DIR))
}

pub async fn sync_custom_packages(working_dir: &Path) -> Result<Vec<CustomPackageSyncReport>> {
    let packages = scan_custom_packages(working_dir)?;
    let mut reports = Vec::new();
    for package in packages {
        reports.push(sync_one_custom_package(&package).await);
    }
    Ok(reports)
}

pub fn ready_runtime_primitives(
    working_dir: &Path,
    disabled_primitives: &[String],
) -> Result<Vec<CustomRuntimePrimitive>> {
    let disabled: std::collections::HashSet<&str> =
        disabled_primitives.iter().map(String::as_str).collect();
    let mut items = Vec::new();
    for package in scan_custom_packages(working_dir)? {
        let status = package_runtime_status(&package);
        if !status.is_ready() {
            continue;
        }
        let Some(python_executable) = status.python_executable() else {
            continue;
        };
        for export in &package.exports {
            if disabled.contains(export.name.as_str()) {
                continue;
            }
            let source = parse_export_source(&export.source_path, &export.callable)?;
            let mut requirements = source.requirements;
            requirements.extend(package.external_commands.iter().cloned());
            if !package.playwright_browsers.is_empty() {
                requirements.extend(
                    package
                        .playwright_browsers
                        .iter()
                        .map(|browser| format!("playwright:{browser}")),
                );
            }
            requirements.sort();
            requirements.dedup();
            items.push(CustomRuntimePrimitive {
                name: export.name.clone(),
                module: export.module.clone(),
                callable: export.callable.clone(),
                python_executable: python_executable.clone(),
                source_path: export.source_path.clone(),
                summary: source.summary,
                call_shape: source.call_shape,
                args: source.args,
                returns: source.returns,
                when_not_to_use: source.when_not_to_use,
                common_mistakes: source.common_mistakes,
                requirements,
            });
        }
    }
    items.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(items)
}

fn load_custom_package(
    working_dir: &Path,
    project_dir: &Path,
    manifest_path: &Path,
) -> Result<CustomPackage> {
    let manifest_text = fs::read_to_string(manifest_path)?;
    let manifest: PyProjectManifest = toml::from_str(&manifest_text)?;
    let lock_path = project_dir.join("uv.lock");
    let lock_path = lock_path.exists().then_some(lock_path);
    let digest = manifest_digest(&manifest_text, lock_path.as_deref())?;
    let mut exports = Vec::new();
    for export in manifest.tool.zerda.exports {
        exports.push(CustomPackageExport {
            source_path: module_source_path(working_dir, &export.module)?,
            name: export.name,
            module: export.module,
            callable: export.callable,
        });
    }
    Ok(CustomPackage {
        project_name: manifest.project.name,
        project_dir: project_dir.to_path_buf(),
        manifest_path: manifest_path.to_path_buf(),
        lock_path,
        digest,
        optional: manifest.tool.zerda.optional,
        dependencies: manifest.project.dependencies,
        external_commands: manifest.tool.zerda.external_commands,
        playwright_browsers: manifest.tool.zerda.playwright_browsers,
        exports,
    })
}

fn module_source_path(working_dir: &Path, module: &str) -> Result<PathBuf> {
    let relative = module.replace('.', "/");
    let file_path = working_dir.join(format!("{relative}.py"));
    if file_path.exists() {
        return Ok(file_path);
    }
    let init_path = working_dir.join(relative).join("__init__.py");
    if init_path.exists() {
        return Ok(init_path);
    }
    anyhow::bail!("module source path not found for {module}");
}

fn manifest_digest(manifest_text: &str, lock_path: Option<&Path>) -> Result<String> {
    let mut hasher = DefaultHasher::new();
    manifest_text.hash(&mut hasher);
    if let Some(path) = lock_path {
        fs::read(path)?.hash(&mut hasher);
    }
    Ok(format!("{:016x}", hasher.finish()))
}

fn sanitize_slug(raw: &str) -> String {
    let mut out = String::new();
    let mut previous_underscore = false;
    for ch in raw.chars().flat_map(char::to_lowercase) {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
            previous_underscore = false;
        } else if !previous_underscore {
            out.push('_');
            previous_underscore = true;
        }
    }
    out.trim_matches('_').to_string()
}

fn command_exists(command: &str) -> bool {
    std::env::var_os("PATH")
        .map(|paths| {
            std::env::split_paths(&paths).any(|path| {
                let candidate = path.join(command);
                if candidate.is_file() {
                    return true;
                }
                if cfg!(windows) {
                    let exe = path.join(format!("{command}.exe"));
                    return exe.is_file();
                }
                false
            })
        })
        .unwrap_or(false)
}

fn ensure_external_commands_available(commands: &[String]) -> Result<()> {
    for command in commands {
        if !command_exists(command) {
            anyhow::bail!("missing external command: {command}");
        }
    }
    Ok(())
}

async fn sync_one_custom_package(package: &CustomPackage) -> CustomPackageSyncReport {
    match sync_one_custom_package_inner(package).await {
        Ok(state) => CustomPackageSyncReport {
            package_name: package.project_name.clone(),
            status: state.status,
            message: state.message,
        },
        Err(error) => {
            let status = if package.optional {
                CustomPackageSyncStatus::PendingSync
            } else {
                CustomPackageSyncStatus::Failed
            };
            let state = CustomPackageStateFile {
                digest: package.digest.clone(),
                status: status.clone(),
                message: error.to_string(),
                python_executable: package.env_python_path(),
                synced_at: Some(chrono::Utc::now().to_rfc3339()),
            };
            let _ = write_state_file(&package.state_path(), &state);
            CustomPackageSyncReport {
                package_name: package.project_name.clone(),
                status,
                message: error.to_string(),
            }
        }
    }
}

async fn sync_one_custom_package_inner(package: &CustomPackage) -> Result<CustomPackageStateFile> {
    ensure_external_commands_available(&package.external_commands)?;

    if !command_exists("uv") {
        anyhow::bail!("uv is required to sync custom primitive packages");
    }

    let base_python = resolve_ptc_python_executable();
    if !Path::new(&base_python).exists() {
        anyhow::bail!("PTC python runtime not found at {base_python}");
    }

    if let Some(parent) = package.env_dir().parent() {
        fs::create_dir_all(parent)?;
    }

    let mut venv = Command::new("uv");
    venv.arg("venv")
        .arg(package.env_dir())
        .arg("--allow-existing")
        .arg("--python")
        .arg(&base_python)
        .arg("--no-project");
    run_command(&mut venv, "create custom primitive venv").await?;

    if !package.dependencies.is_empty() {
        let mut install = Command::new("uv");
        install
            .arg("pip")
            .arg("install")
            .arg("--python")
            .arg(package.env_python_path());
        for dependency in &package.dependencies {
            install.arg(dependency);
        }
        run_command(&mut install, "install custom primitive dependencies").await?;
    }

    if !package.playwright_browsers.is_empty() {
        let mut playwright = Command::new(package.env_python_path());
        playwright.arg("-m").arg("playwright").arg("install");
        for browser in &package.playwright_browsers {
            playwright.arg(browser);
        }
        run_command(&mut playwright, "install playwright browsers").await?;
    }

    let state = CustomPackageStateFile {
        digest: package.digest.clone(),
        status: CustomPackageSyncStatus::Ready,
        message: "ready".to_string(),
        python_executable: package.env_python_path(),
        synced_at: Some(chrono::Utc::now().to_rfc3339()),
    };
    write_state_file(&package.state_path(), &state)?;
    Ok(state)
}

fn resolve_ptc_python_executable() -> String {
    std::env::var(PTC_PYTHON_ENV)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_PTC_PYTHON.to_string())
}

async fn run_command(command: &mut Command, purpose: &str) -> Result<()> {
    let output = command.output().await?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let detail = if !stderr.is_empty() {
        stderr
    } else if !stdout.is_empty() {
        stdout
    } else {
        format!("exit status {}", output.status)
    };
    anyhow::bail!("{purpose} failed: {detail}");
}

struct ExportSourceMetadata {
    summary: String,
    call_shape: String,
    args: String,
    returns: String,
    when_not_to_use: String,
    common_mistakes: String,
    requirements: Vec<String>,
}

fn parse_export_source(path: &Path, callable: &str) -> Result<ExportSourceMetadata> {
    let text = fs::read_to_string(path)?;
    let signature = parse_signature(&text, callable).unwrap_or_else(|| callable.to_string());
    let docstring = extract_docstring(&text, callable);
    Ok(ExportSourceMetadata {
        summary: extract_section(&docstring, "What it does")
            .or_else(|| first_non_empty_line(&docstring))
            .unwrap_or_else(|| callable.to_string()),
        call_shape: signature,
        args: extract_section(&docstring, "Args").unwrap_or_default(),
        returns: extract_section(&docstring, "Output Contract").unwrap_or_default(),
        when_not_to_use: extract_section(&docstring, "When NOT to use").unwrap_or_default(),
        common_mistakes: extract_section(&docstring, "Common Mistakes").unwrap_or_default(),
        requirements: infer_requirements(&text),
    })
}

fn parse_signature(text: &str, callable: &str) -> Option<String> {
    let needle = format!("async def {callable}");
    let start = text.find(&needle)?;
    let tail = &text[start..];
    let line = tail.lines().next()?.trim();
    let signature = line
        .strip_prefix("async def ")
        .unwrap_or(line)
        .trim_end_matches(':')
        .trim();
    Some(signature.to_string())
}

fn extract_docstring(text: &str, callable: &str) -> String {
    let needle = format!("async def {callable}");
    let Some(start) = text.find(&needle) else {
        return String::new();
    };
    let tail = &text[start..];
    let Some(after_signature_offset) = tail.find('\n') else {
        return String::new();
    };
    let after_signature = tail[after_signature_offset + 1..].trim_start();
    for quote in ["\"\"\"", "'''"] {
        if let Some(rest) = after_signature.strip_prefix(quote) {
            if let Some(end) = rest.find(quote) {
                return rest[..end].trim().to_string();
            }
        }
    }
    String::new()
}

fn extract_section(docstring: &str, title: &str) -> Option<String> {
    extract_doc_sections(docstring).remove(title)
}

fn extract_doc_sections(docstring: &str) -> std::collections::HashMap<String, String> {
    let mut sections = std::collections::HashMap::new();
    let mut current: Option<String> = None;
    let mut lines = Vec::new();
    for raw in docstring.lines() {
        let line = raw.trim();
        if line.starts_with('[') && line.ends_with(']') && line.len() > 2 {
            if let Some(title) = current.take() {
                let value = lines.join("\n").trim().to_string();
                if !value.is_empty() {
                    sections.insert(title, value);
                }
            }
            current = Some(
                line.trim_start_matches('[')
                    .trim_end_matches(']')
                    .to_string(),
            );
            lines.clear();
            continue;
        }
        if current.is_some() && !line.is_empty() {
            lines.push(line.to_string());
        }
    }
    if let Some(title) = current {
        let value = lines.join("\n").trim().to_string();
        if !value.is_empty() {
            sections.insert(title, value);
        }
    }
    sections
}

fn first_non_empty_line(text: &str) -> Option<String> {
    text.lines()
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::sync::{LazyLock, Mutex};
    use uuid::Uuid;

    static CUSTOM_PACKAGE_ENV_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

    fn temp_root(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("zerda-{name}-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).expect("create temp root");
        root
    }

    fn write_file(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create parent");
        }
        fs::write(path, content).expect("write file");
    }

    #[test]
    fn scans_custom_package_manifest_and_exports() {
        let root = temp_root("custom-package-scan");
        write_file(
            &root.join("custom_primitives/demo_pkg/pyproject.toml"),
            r#"
[project]
name = "demo-pkg"
version = "0.1.0"
dependencies = ["httpx==0.27.0"]

[tool.zerda]
external-commands = ["demo-cli"]
playwright-browsers = ["chromium"]

[[tool.zerda.exports]]
name = "demo_call"
module = "custom_primitives.demo_pkg.impl"
callable = "demo_call"
"#,
        );
        write_file(
            &root.join("custom_primitives/demo_pkg/impl.py"),
            r#"
async def demo_call(query: str) -> dict[str, object]:
    return {"status": "ok"}
"#,
        );

        let packages = scan_custom_packages(&root).expect("scan packages");
        assert_eq!(packages.len(), 1);
        let package = &packages[0];
        assert_eq!(package.project_name, "demo-pkg");
        assert!(!package.optional);
        assert_eq!(package.dependencies, vec!["httpx==0.27.0"]);
        assert_eq!(package.external_commands, vec!["demo-cli"]);
        assert_eq!(package.playwright_browsers, vec!["chromium"]);
        assert_eq!(package.exports.len(), 1);
        let export = &package.exports[0];
        assert_eq!(export.name, "demo_call");
        assert_eq!(export.module, "custom_primitives.demo_pkg.impl");
        assert_eq!(export.callable, "demo_call");
        assert_eq!(
            export.source_path,
            root.join("custom_primitives/demo_pkg/impl.py")
        );
    }

    #[test]
    fn runtime_ready_packages_require_matching_ready_state() {
        let _guard = CUSTOM_PACKAGE_ENV_LOCK
            .lock()
            .expect("custom package env lock");
        let root = temp_root("custom-package-ready");
        write_file(
            &root.join("custom_primitives/demo_pkg/pyproject.toml"),
            r#"
[project]
name = "demo-pkg"
version = "0.1.0"

[tool.zerda]

[[tool.zerda.exports]]
name = "demo_call"
module = "custom_primitives.demo_pkg.impl"
callable = "demo_call"
"#,
        );
        write_file(
            &root.join("custom_primitives/demo_pkg/impl.py"),
            "async def demo_call() -> dict[str, object]:\n    return {\"status\": \"ok\"}\n",
        );

        let cache_root = root.join(".cache");
        unsafe {
            std::env::set_var("ZERDA_CUSTOM_PRIMITIVES_CACHE_DIR", &cache_root);
        }
        let packages = scan_custom_packages(&root).expect("scan packages");
        assert_eq!(packages.len(), 1);
        let package = &packages[0];
        assert!(!package_runtime_status(package).is_ready());

        fs::create_dir_all(package.env_dir()).expect("create env dir");
        write_file(&package.env_python_path(), "");
        let state = CustomPackageStateFile {
            digest: package.digest.clone(),
            status: CustomPackageSyncStatus::Ready,
            message: "ready".to_string(),
            python_executable: package.env_python_path(),
            synced_at: Some("2026-04-11T00:00:00Z".to_string()),
        };
        write_state_file(&package.state_path(), &state).expect("write state");

        let status = package_runtime_status(package);
        assert!(status.is_ready());
        assert_eq!(status.python_executable(), Some(package.env_python_path()));
        unsafe {
            std::env::remove_var("ZERDA_CUSTOM_PRIMITIVES_CACHE_DIR");
        }
    }

    #[test]
    fn ready_runtime_primitives_only_include_ready_packages() {
        let _guard = CUSTOM_PACKAGE_ENV_LOCK
            .lock()
            .expect("custom package env lock");
        let root = temp_root("custom-runtime-primitives");
        write_file(
            &root.join("custom_primitives/demo_pkg/pyproject.toml"),
            r#"
[project]
name = "demo-pkg"
version = "0.1.0"

[tool.zerda]

[[tool.zerda.exports]]
name = "demo_call"
module = "custom_primitives.demo_pkg.impl"
callable = "demo_call"
"#,
        );
        write_file(
            &root.join("custom_primitives/demo_pkg/impl.py"),
            r#"
async def demo_call(query: str) -> dict[str, object]:
    """
    [What it does]
    Demo call.

    [Args]
    query: Demo query.

    [Output Contract]
    data.answer contains the resolved answer text.

    [When NOT to use]
    Skip for empty input.

    [Common Mistakes]
    Forgetting the query field.
    """
    return {"status": "ok"}
"#,
        );

        let cache_root = root.join(".cache");
        unsafe {
            std::env::set_var("ZERDA_CUSTOM_PRIMITIVES_CACHE_DIR", &cache_root);
        }

        assert!(ready_runtime_primitives(&root, &[])
            .expect("runtime primitives")
            .is_empty());

        let packages = scan_custom_packages(&root).expect("scan packages");
        assert_eq!(packages.len(), 1);
        let package = &packages[0];
        fs::create_dir_all(package.env_dir()).expect("create env dir");
        write_file(&package.env_python_path(), "");
        write_state_file(
            &package.state_path(),
            &CustomPackageStateFile {
                digest: package.digest.clone(),
                status: CustomPackageSyncStatus::Ready,
                message: "ready".to_string(),
                python_executable: package.env_python_path(),
                synced_at: Some("2026-04-11T00:00:00Z".to_string()),
            },
        )
        .expect("write state");
        assert!(package_runtime_status(package).is_ready());

        let primitives = ready_runtime_primitives(&root, &[]).expect("runtime primitives");
        assert_eq!(primitives.len(), 1);
        assert_eq!(primitives[0].name, "demo_call");
        assert_eq!(primitives[0].summary, "Demo call.");
        assert!(primitives[0].call_shape.contains("demo_call("));
        assert!(primitives[0].returns.contains("data.answer"));

        unsafe {
            std::env::remove_var("ZERDA_CUSTOM_PRIMITIVES_CACHE_DIR");
        }
    }

    #[test]
    fn optional_package_failures_downgrade_to_pending_sync() {
        let package = CustomPackage {
            project_name: "optional-demo".to_string(),
            project_dir: PathBuf::from("/tmp/optional-demo"),
            manifest_path: PathBuf::from("/tmp/optional-demo/pyproject.toml"),
            lock_path: None,
            digest: "digest".to_string(),
            optional: true,
            dependencies: Vec::new(),
            external_commands: vec!["missing-cmd".to_string()],
            playwright_browsers: Vec::new(),
            exports: Vec::new(),
        };

        let report = tokio::runtime::Runtime::new()
            .expect("runtime")
            .block_on(sync_one_custom_package(&package));
        assert_eq!(report.status, CustomPackageSyncStatus::PendingSync);
        assert_eq!(report.message, "missing external command: missing-cmd");
    }
}
