# PTC Runtime Architecture

## Overview

Zerda uses a single-assistant runtime with asynchronous PTC jobs.

- The model handles dialogue directly.
- Python-based mechanical work is expressed as `<PTC_TOOL_CALLING>` blocks.
- Primitive discovery is expressed as Rust-handled `RUST_CALL` blocks.
- The host intercepts those blocks, launches detached Python jobs, and injects completion results back into the same session.
- There is no provider-level tool calling layer, no MCP integration, and no Skills subsystem.

## Turn Flow

1. `src/main.rs` loads config, identity, providers, and optional STT.
2. `src/runner.rs` builds the user turn and appends runtime job state or conversation summary when needed.
3. `src/agent.rs` sends history to the active provider.
4. `src/ptc/stream_interceptor.rs` splits visible assistant text from hidden PTC XML.
5. `src/ptc/parser.rs` parses exact `<PTC_TOOL_CALLING>` and Rust-native `RUST_CALL` blocks.
6. `src/ptc/primitive_index.rs` scans internal, external, and custom primitives for metadata.
7. `src/ptc/job_manager.rs` either launches Python jobs or serves Rust-native primitive discovery results.
8. Completed job results are injected back as runtime-originated `user` messages.

## Why PTC

- Keeps provider requests simple: only `system`, `user`, and `assistant` messages.
- Avoids provider-specific function calling or tool schema drift.
- Moves execution complexity into bounded Python jobs with artifacts and replayability.
- Keeps the main conversation loop responsive while long-running work continues in the background.

## Artifacts

PTC jobs write artifacts under `~/.zerda/ptc_jobs/<YYYYMMDD>/<job>/`:

- `script.py`
- `out.json`
- `log.txt`
- `telemetry.jsonl`
- `status.json`
- `meta.json`
