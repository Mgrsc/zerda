use std::sync::LazyLock;

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
    max_prompt_chars: usize,
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
        - After adding or removing MCP servers or skills, perform a light reload (/reload command)"
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
    if prompt.len() > max_prompt_chars {
        tracing::warn!(
            "System prompt exceeds {max_prompt_chars} chars ({} chars)",
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
