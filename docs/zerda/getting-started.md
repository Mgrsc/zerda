# Getting Started

## Installation

Zerda is a Rust application. Build from source:

```bash
cargo build --release
```

The binary is located at `target/release/zerda`.

## Quick Start

1. Copy `.env.example` to `.env` and fill in your API keys.
2. Create a minimal config file at `~/.zerda/zerda.toml`:

```toml
[providers.openai]
type = "openai_chat"
api_key = "${OPENAI_API_KEY}"

[agent]
primary_model = { name = "openai@gpt-4o" }
```

3. Run Zerda:

```bash
# Interactive mode
zerda

# Single-turn mode
zerda run -m "Hello"

# Resume a previous session
zerda run --resume

# Start background services (Telegram bot)
zerda serve
```

## Config File Resolution Order

1. `--config` / `-c` CLI argument
2. `$ZERDA_CONFIG` environment variable
3. `~/.zerda/zerda.toml` (default fallback)

## Directory Structure

Zerda uses `~/.zerda/` as its home directory:

```
~/.zerda/
├── zerda.toml          # Main configuration
├── mcp.toml            # MCP server configurations (optional)
├── identity.md         # Agent personality definition
├── memory/
│   └── MEMORY.md       # Persistent long-term memory
├── sessions/           # Conversation history per session
├── skills/             # Skill definitions
│   └── skill-name/
│       └── SKILL.md
└── executor_jobs/      # Executor artifacts
    └── YYYYMMDD/
        └── HHMMSS_task-slug/
```

## Generate Full Config Template

To see all available configuration options with documentation:

```bash
zerda config generate
```

This prints the comprehensive `zerda.toml.full` template to stdout.

## Validate Configuration

```bash
zerda config validate
```

Checks the active config file for errors and exits.
