# Commands Reference

## CLI Commands

### `zerda`

Interactive multi-turn chat:

```bash
zerda
zerda --config /path/to/zerda.toml
```

### `zerda run`

One-shot or resumed execution:

```bash
zerda run -m "List files in this repo"
zerda run --resume
zerda run --resume <session_id>
```

### `zerda serve`

Start configured remote channels:

```bash
zerda serve
```

### `zerda config`

```bash
zerda config generate
zerda config validate
```

## Interactive Commands

| Command | Description |
|---|---|
| `/help` | Show available commands |
| `/model` | View current active model and providers |
| `/model <provider_id>@<model_name>` | Switch model at runtime |
| `/model <provider_id> list` | List available models for a provider |
| `/clear` | Clear conversation history and token usage |
| `/compact` | Force context compression |
| `/status` | Display token usage and runtime state |
| `/jobs` | List current session PTC jobs |
| `/job <id>` | Inspect one PTC job |
| `/cancel-job <id>` | Cancel a running PTC job |
| `/cancel` | Cancel the current running turn |
| `/exit` or `/quit` | Exit the interactive session |

## Busy Turn Behavior

- `/status`, `/jobs`, `/job <id>`, and `/cancel-job <id>` execute immediately while a turn is streaming.
- `/compact` is queued and runs after the active turn finishes.
- `/clear` and `/model <provider>@<model>` cancel the active turn first, then execute.
- `/model` without arguments still returns immediately.
- `/model <provider> list` is queued while a turn is active because provider lookup still needs mutable runtime state.

## Model Switching

Temporary session switch:

```text
/model openai@gpt-4o-mini
/model anthropic@claude-4-sonnet
```

Persistent switch:

```toml
[agent.primary_model]
model = "openai@gpt-4o-mini"
vision = true
```

Then restart Zerda.
