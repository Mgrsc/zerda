use std::sync::LazyLock;

use crate::config;

const SYSTEM_RULES: &str = include_str!("prompts/system_rules.md");

struct EnvInfo {
    os: String,
    shell: String,
    package_manager: String,
}

static ENV_INFO: LazyLock<EnvInfo> = LazyLock::new(|| EnvInfo {
    os: read_os_pretty_name(),
    shell: read_default_shell(),
    package_manager: detect_package_manager(),
});

pub fn build_system_prompt_parts(
    identity: Option<&str>,
    channel_supplement: Option<&str>,
) -> Vec<String> {
    let mut parts: Vec<String> = Vec::new();

    if let Some(id) = identity {
        parts.push(id.to_string());
    }

    parts.push(SYSTEM_RULES.trim_end().to_string());

    parts.push(build_env_block());

    if let Some(supplement) = channel_supplement {
        parts.push(supplement.to_string());
    }

    let prompt_len =
        parts.iter().map(String::len).sum::<usize>() + (parts.len().saturating_sub(1) * 2);
    tracing::trace!(
        "System prompt parts: {}, total size: {} chars",
        parts.len(),
        prompt_len
    );
    if prompt_len > config::MAX_PROMPT_CHARS {
        tracing::warn!(
            "System prompt exceeds {} chars ({} chars)",
            config::MAX_PROMPT_CHARS,
            prompt_len
        );
    }
    parts
}

fn build_env_block() -> String {
    let hostname = std::env::var("HOSTNAME")
        .or_else(|_| std::fs::read_to_string("/etc/hostname").map(|s| s.trim().to_string()))
        .unwrap_or_else(|_| "unknown".to_string());

    let cwd = std::env::current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_default();

    let env = &*ENV_INFO;

    let mut env_block = format!(
        "<env>\nhostname: {hostname}\nworking_directory: {cwd}\nos: {}\nshell: {}",
        env.os, env.shell
    );
    if !env.package_manager.is_empty() {
        env_block.push_str(&format!("\npackage_manager: {}", env.package_manager));
    }
    env_block.push_str("\n</env>");
    env_block
}

pub(crate) fn read_os_pretty_name() -> String {
    std::fs::read_to_string("/etc/os-release")
        .ok()
        .and_then(|content| {
            content
                .lines()
                .find(|line| line.starts_with("PRETTY_NAME="))
                .map(|line| {
                    line.trim_start_matches("PRETTY_NAME=")
                        .trim_matches('"')
                        .to_string()
                })
        })
        .unwrap_or_else(|| std::env::consts::OS.to_string())
}

pub(crate) fn read_default_shell() -> String {
    std::env::var("SHELL")
        .ok()
        .and_then(|s| s.rsplit('/').next().map(String::from))
        .unwrap_or_else(|| "sh".to_string())
}

fn detect_package_manager() -> String {
    const CANDIDATES: &[&str] = &[
        "paru", "yay", "pacman", "apt", "apt-get", "dnf", "zypper", "brew", "nix", "apk", "emerge",
    ];
    for name in CANDIDATES {
        if std::process::Command::new("which")
            .arg(name)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
        {
            return (*name).to_string();
        }
    }
    String::new()
}
