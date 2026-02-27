# Memory System

## Components

### MEMORY.md (Long-Term Memory)

- Location: `~/.zerda/memory/MEMORY.md`
- Persistent across sessions
- Max size: configurable via `max_memory_file_size` (default: 102400 bytes / ~100KB)
- Max tokens loaded per turn: `max_memory_tokens` (default: 2000, approximately `max_memory_tokens * 4` characters)
- Accessed by the agent via memory tool (read/append)

### user.md (User Context)

- Location: `~/.zerda/memory/user.md`
- Loaded each turn if the file exists
- Injected as `<user-context>` block in user messages
- Used for user preferences, background info, persistent metadata

### Sessions

- Location: `~/.zerda/sessions/`
- Per-session conversation history stored as JSON
- Named: `<session_id>.json` (CLI) or `channel-<hex_encoded_id>.json` (Telegram)
- Auto-saved after each turn
- Auto-cleanup: sessions older than `session_cleanup_days` (default: 7) deleted on startup

### Executor Jobs

- Location: `~/.zerda/executor_jobs/<YYYYMMDD>/<HHMMSS>_<task_slug>/`
- Artifacts: `script.py`, `log.txt`, `out.json`, `telemetry.jsonl`, `meta.json`
- Used for replay and postmortem analysis

### Compaction Artifacts

- Location: `~/.zerda/compaction/`
- Full transcript snapshots saved before compression
- Enables lossless recovery if needed

## Context Compression

### Auto-Compression

Triggered when non-system message count exceeds `max_history` (default: 30). The fast model (or primary model) summarizes the conversation into a compact summary.

### Manual Compression

Use the `/compact` command in interactive mode to force compression at any time.

### How It Works

1. Full transcript is persisted to `compaction/` directory
2. Conversation is summarized by the compression model
3. Summary is injected as `<conversation-summary>` in subsequent messages
4. Original messages are replaced with the summary

## Context Assembly

Each user message is assembled with these content blocks (in order):

1. Skills index (available skills listing)
2. Todo reminders (pending tasks)
3. User context (from `user.md`)
4. Conversation summary (if compressed)
5. Current timestamp
6. User input (text, images, or multipart)

Each block is self-contained and removable without breaking others.

## Token Budget

Set `max_budget_tokens` in agent config to limit total token usage per session. When exceeded, Zerda stops processing. Default: unlimited.
