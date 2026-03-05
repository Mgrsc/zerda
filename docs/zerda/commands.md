# Commands Reference

## CLI Commands

### `zerda` (default: interactive mode)

Enter interactive chat mode for multi-turn conversations. Session-based with automatic persistence.

```bash
zerda
zerda --config /path/to/zerda.toml
```

### `zerda run`

Execute in specific modes:

```bash
# Single-turn mode: execute one instruction and exit
zerda run -m "What is the weather today?"

# Resume the latest session
zerda run --resume

# Resume a specific session by ID
zerda run --resume <session_id>
```

### `zerda serve`

Start background services (Telegram bot, channel listeners). Runs indefinitely.

```bash
zerda serve
```

### `zerda config`

```bash
# Print full example config with all options documented
zerda config generate

# Validate the active config and exit
zerda config validate
```

## Interactive Commands (in chat)

These commands are available during an interactive session:

| Command | Description |
|---------|-------------|
| `/help` | Show available commands |
| `/model` | View current active model and available providers |
| `/model <provider_id>@<model_name>` | Switch model at runtime (e.g., `/model openai@gpt-4o`) |
| `/model <provider_id> list` | List available models for a provider |
| `/clear` | Clear conversation history and reset token usage |
| `/compact` | Force context compression manually |
| `/status` | Display session status (tokens, tools, skills, todos, system info) |
| `/cancel` | Cancel the currently running turn |
| `/exit` or `/quit` | Exit the interactive session |

## Switching Models

### Runtime Switch (temporary, current session only)

```
/model openai@gpt-4o-mini
/model anthropic@claude-3-5-sonnet-20241022
```

This takes effect immediately for the current session without needing a config reload.

### Persistent Switch (edit config)

Change `primary_model` in `zerda.toml`:

```toml
[agent.primary_model]
model = "openai@gpt-4o-mini"
vision = true
```

Then either restart Zerda or use the `reload` tool with `mode='full'`.

### List Available Models

```
/model openai list
```

Queries the provider's model listing endpoint. Not all providers support this (Anthropic may return 404).

## Reload Configuration

The `reload` tool (called by the agent) supports two modes:

- **`light` mode**: Reloads MCP servers, skills, identity/prompts in-place. Fast, no restart needed.
- **`full` mode**: Full process restart. Required for changes to providers, channels, STT, TTS settings.

The agent can be asked to reload: "reload your config" or "reload MCP servers".
