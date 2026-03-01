# Configuration Reference

## Providers

Define LLM providers under `[providers.<id>]`. Multiple providers can coexist.

```toml
[providers.openai]
type = "openai_chat"          # "openai_chat" | "openai_responses" | "anthropic"
api_key = "${OPENAI_API_KEY}"
base_url = "https://api.openai.com/v1"  # Optional, defaults to official endpoint
# extra_headers = { "X-Custom" = "value" }  # Optional

[providers.openai.retry]
max_retries = 3               # Default: 3
base_delay_ms = 2000          # Default: 2000
max_delay_ms = 30000          # Default: 30000
connect_timeout_secs = 10     # Default: 10
request_timeout_secs = 120    # Default: 120
```

### Supported Provider Types

| Type | Protocol | Example Models |
|------|----------|----------------|
| `openai_chat` | OpenAI Chat Completions API | gpt-4o, gpt-4o-mini, gpt-5.2 |
| `openai_responses` | OpenAI Responses API | Same, with extended reasoning support |
| `anthropic` | Anthropic Messages API | claude-3-5-sonnet, claude-4-sonnet |

## Agent Settings

```toml
[agent]
# Primary model (required) - format: provider_id@model_name
primary_model = { name = "openai@gpt-4o", vision = true, temperature = 0.7 }

# Fast model for compression and subagent (optional, falls back to primary)
fast_model = { name = "openai@gpt-4o-mini" }

# Model parameters (all optional)
# primary_model.vision = true        # Enable image processing (default: true)
# primary_model.temperature = 0.7    # Sampling temperature (0.0-2.0)
# primary_model.top_p = 1.0          # Top-p sampling (0.0-1.0)
# primary_model.max_tokens = 4096    # Output token limit

max_iterations = 10           # Max tool calls per turn (default: 10, min: 10)
max_history = 30              # Message count before auto-compression (default: 30)
max_tool_output_chars = 30000 # Truncate tool results exceeding this (default: 30000)
max_memory_tokens = 2000      # Max tokens for memory content (default: 2000)
max_memory_file_size = 102400 # MEMORY.md max size in bytes (default: 102400)
tool_timeout = 300            # Tool execution timeout in seconds (default: 300)
disabled_primitives = []      # Primitive blacklist; all enabled when empty
session_cleanup_days = 7      # Auto-clean sessions older than N days (default: 7)
show_usage = false            # Print token usage after each turn (default: false)
# max_budget_tokens = 100000  # Session token budget, unlimited by default
identity_path = "~/.zerda/identity.md"  # System role definition (default)
```

## Channels

```toml
[[channels]]
name = "telegram"
token = "${TELEGRAM_BOT_TOKEN}"
allowed_users = ["*"]           # ["*"] = all users, or list specific user IDs
max_message_length = 4096       # Default: 4096
```

Currently only `telegram` channel is supported.

## MCP Servers

Defined in `mcp.toml` (must be in the same directory as the active `zerda.toml`).

```toml
# Stdio transport (local subprocess)
[[mcp]]
name = "filesystem"
transport = "stdio"
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "/home/user"]
[mcp.env]
NODE_ENV = "production"

# Streamable HTTP transport (remote)
[[mcp]]
name = "remote-api"
transport = "streamable-http"
url = "https://example.com/mcp"
```

Changes to MCP servers can be reloaded with `reload` tool in `light` mode.

## TTS (Text-to-Speech)

```toml
[tts]
provider = "minimax"            # Currently only "minimax" supported
api_key = "${MINIMAX_API_KEY}"
model = "speech-2.8-hd"        # Default: "speech-2.8-hd"
# voice_id = "female-shaonv"   # Optional voice identifier
```

Leave `provider` empty or omit the section to disable TTS.

## STT (Speech-to-Text)

```toml
[stt]
provider = "groq"               # Currently only "groq" supported
api_key = "${GROQ_API_KEY}"
model = "whisper-large-v3-turbo" # Default: "whisper-large-v3-turbo"
```

Used for Telegram voice message transcription.

## Logging

```toml
[log]
level = "info"   # "trace" | "debug" | "info" | "warn" | "error"
```

## Environment Variable Expansion

Config values support `${VAR}` syntax for environment variable expansion. This is processed before TOML parsing, so it works in all string values.

```toml
api_key = "${OPENAI_API_KEY}"
token = "${TELEGRAM_BOT_TOKEN}"
```
