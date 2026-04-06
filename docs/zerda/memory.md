# Memory System

## Local State Model

Zerda keeps only local recoverable state:

- session history
- conversation compaction summaries
- compaction transcript snapshots
- PTC job artifacts
- conversation-scoped browser session state
- EMA SQLite turn journal and future memory state

EMA entity model:

- Zerda currently treats memory as single-user global memory.
- All sessions write into and recall from the same logical memory entity.

EMA memory classes:

- personal memory: events, commitments, preferences, profile facts, constraints, and personal insights
- operational memory: reusable execution procedures, failure patterns, and operational insights distilled from completed runtime/problem-solving turns

## Runtime Persistence

### Sessions

- Path: `~/.zerda/sessions/`
- One JSON file per session
- Auto-saved after completed turns

### PTC Jobs

- Path: `~/.zerda/ptc_jobs/<YYYYMMDD>/<job>/`
- Contains `script.py`, `out.json`, `log.txt`, `telemetry.jsonl`, `status.json`, and metadata

### Compaction Artifacts

- Path: `~/.zerda/memory/compaction/`
- Full transcript snapshots written before compression

### Browser State

- Path: `~/.zerda/agent_browser_state/*.json`
- Stores the default browser session bound to each Zerda conversation

### EMA SQLite

- Path: `~/.zerda/memory/ema.sqlite3`
- Stores turn journal rows, structured memory entries, links, recall logs, and maintenance metadata

## Context Assembly

Each user turn may include these blocks in order:

1. Runtime job state
2. EMA memory blocks
3. Conversation summary
4. Current timestamp
5. User input

EMA note:

- Hot-path recall injects facts, insights, failure patterns, and procedures before the current user input.
- Ordinary open-ended recall prioritizes profile facts, commitments, preferences, constraints, events, and insights.
- Completed human turns and runtime-backed problem-solving turns are journaled after the reply is sent.
- Memory maintenance is buffered: extraction and consolidation run only when pending turns reach the backlog threshold or the oldest pending turn exceeds the age threshold.
- Personal durable memory only accepts proposals backed by exact quotes copied from user-authored messages in that completed turn.
- Operational durable memory only accepts reusable procedures or failure patterns backed by exact assistant/runtime quotes from the completed turn.
- Personal durable memory does not store procedures.
- Operational maintenance can consolidate multiple procedures or failure patterns into higher-level operational insights.
- Failure patterns are recalled only for troubleshooting-style queries and are not mixed into ordinary memory recall by default.
- Facts without verified evidence are rejected before persistence and do not participate in consolidation.
- Active memories can be archived when they stay low-value and untouched for long enough; heavily reused memories decay more slowly.

## Conflict Semantics

- `active`: current version participates in recall
- `superseded`: replaced by a newer version on the same semantic axis
- `obsolete`: older insight replaced by a newer abstraction
- `expired`: time-bounded event or commitment naturally passed
- `cancelled`: explicitly cancelled event, commitment, or rule
- `archived`: cold non-active memory removed from the active index

Conflict handling is semantic, not just textual:

- `version_key` is still used
- a coarser `axis` is also derived so newer memories can replace or cancel older ones without relying on exact full-key matches
- event, procedure, and failure-pattern memories are treated as distinct conflict domains
