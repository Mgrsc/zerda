# Planner-Executor Architecture

## Overview

Zerda uses a Planner-Executor architecture that separates reasoning from mechanical execution.

## Planner

The Planner is the main reasoning agent. It:

- Understands user intent and decomposes goals into tasks
- Collects information (searches, reads, queries)
- Delegates concrete operations to the Executor via the `delegate_to_executor` tool
- Synthesizes results from executor runs into coherent responses
- Never executes mechanical work directly

## Executor

The Executor handles mechanical task execution:

- Receives goal-oriented briefs from the Planner
- Writes and runs Python scripts to accomplish tasks
- Stores artifacts in `~/.zerda/executor_jobs/<YYYYMMDD>/<HHMMSS>_<task_slug>/`
- Returns standardized results with status, data, error info

### Executor Artifacts

Each executor job produces:
- `script.py` - Generated Python code
- `log.txt` - Combined stdout/stderr
- `out.json` - Key results in structured format
- `telemetry.jsonl` - Execution metrics
- `meta.json` - Job metadata

### Executor Return Contract

```json
{
  "status": "ok|partial|error|timeout",
  "data": { ... },
  "error_code": "string",
  "error_message": "string",
  "retryable": true
}
```

## Delegation Brief Format

When the Planner delegates to the Executor, it provides:
- **GOAL**: What needs to be accomplished
- **INPUT**: Data and context
- **CONSTRAINTS**: Limitations and requirements
- **DONE_WHEN**: Completion criteria
- **RETURN**: What to return

## Benefits

- **KV-Cache Friendly**: Static system prompt with append-only history maximizes prefix cache hits
- **Context Rot Resistance**: Mechanical errors isolated to executor artifacts, not polluting planner context
- **Token Efficiency**: ~80% lower token usage vs traditional ReAct in observed samples
- **Concurrency**: Planner can fan-out multiple independent executor jobs
