use std::sync::LazyLock;

use crate::config;

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

pub fn build_system_prompt(
    identity: Option<&str>,
    channel_supplement: Option<&str>,
) -> String {
    let mut parts: Vec<String> = Vec::new();

    if let Some(id) = identity {
        parts.push(id.to_string());
    }

    parts.push(
        "## Rules\n\
        - NEVER give time estimates or predictions\n\
        - Always respond in the user's language\n\
        - When you need to call a tool, call it directly without asking for permission\n\
        - After editing zerda.toml, call the reload tool to apply the configuration\n\
        - Record important information to MEMORY.md using the memory tool; read it when needed\n\
        - After adding or removing MCP servers or skills, perform a light reload (/reload command)\n\
        - Prefer the shortest path: do not build a perfect solution in one pass; get core results first\n\
        - If a tool fails 2 times in a row, abandon that approach and switch to a simpler alternative or ask the user\n\
        - Before each tool call, confirm it advances the goal; if not, stop and reassess\n\
        - Do not repeatedly attempt the same approach for the same problem\n\
        - Never run long-lived or blocking commands in the foreground (e.g., dev servers, file watchers, tail -f)\n\
        - For long-running tasks, start them in the background with logs and report PID, log path, stop command, and a health check result\n\
        - Foreground shell commands must be short-lived and bounded with an explicit timeout\n\
        - If you are unsure whether a command may block, ask the user before running it"
            .to_string(),
    );

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
    parts.push(env_block);

    if let Some(supplement) = channel_supplement {
        parts.push(supplement.to_string());
    }

    let prompt = parts.join("\n\n");
    tracing::debug!("System prompt size: {} chars", prompt.len());
    if prompt.len() > config::MAX_PROMPT_CHARS {
        tracing::warn!(
            "System prompt exceeds {} chars ({} chars)",
            config::MAX_PROMPT_CHARS,
            prompt.len()
        );
    }
    prompt
}

fn read_os_pretty_name() -> String {
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

fn read_default_shell() -> String {
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
