# Built-in Tools

## Planner-Level Tools

These tools are available to the Planner agent.

### `reload`

Reload configuration without restarting.

- `mode='light'`: Reload MCP servers, skills, identity/prompts in-place (fast)
- `mode='full'`: Full process restart for provider/channel/STT/TTS changes

### `skill`

Activate a skill by name to retrieve its full instructions.

- `name`: Skill name (required)
- `args`: Optional arguments passed to the skill

Returns the complete SKILL.md content with `$ARGUMENTS` placeholder substituted.

### `todo`

Session task management for complex multi-step work.

- `action='add'`, `text`: Add a new task
- `action='edit'`, `id`, `text`: Edit an existing task
- `action='done'`, `id`: Mark a task complete
- `action='list'`: List all tasks
- `action='clear'`: Clear all tasks

Pending tasks are automatically reminded before each user message to resist attention collapse.

### `tts`

Convert text to speech audio. Available only when TTS provider is configured.

- `text`: Text to convert (required)
- Returns: `<voice>/tmp/zerda_tts_<id>.ogg</voice>` marker

### `delegate_to_executor`

Delegate mechanical work to the Executor agent. Available when subagent provider is configured.

- `instruction`: Structured instruction (required), format: `ACTION(param=value, ...) -> {return_fields}`

### `search_zerda_documents`

Semantic search of Zerda's own documentation. Available when `[docs_search]` is enabled and valid in `zerda.toml`.

On first startup, Zerda automatically indexes `docs_search.docs_dir` into Qdrant collection `docs_search.collection`. Later startups sync incrementally.

- `query`: Search query (required)
- `max_results`: 1-10, default 5

## Executor-Level Tools

These tools are available only within executor jobs (called by the Executor, not directly by the Planner).

### `shell`

Execute shell commands with timeout.

- `command`: Shell command to execute

### `execute_python_script`

Write and run Python code with managed artifacts.

- `code`: Pure Python code to execute

Environment variables injected into the script:
- `EXECUTOR_OUT_PATH`: Path for structured output JSON
- `EXECUTOR_LOG_PATH`: Path for log file
- `EXECUTOR_TELEMETRY_PATH`: Path for telemetry JSONL
- `EXECUTOR_PRIMITIVES_PY_ROOT`: Root path for code primitives
- `EXECUTOR_DISABLED_PRIMITIVES`: JSON array or comma-separated primitive blacklist
