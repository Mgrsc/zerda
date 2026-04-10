# Code Primitives

## Overview

Code primitives are prewritten async Python functions injected into PTC job scripts. They provide reliable implementations for common operations, while Rust handles primitive discovery metadata and lookup.

## Location

Core primitives are located in `code_primitives/python/`:

```
code_primitives/python/
├── bootstrap.py              # Runtime injection entry point
├── sitecustomize.py          # Global Python startup injection hook
├── primitives/
│   ├── types.py              # Standard return types
│   ├── base.py               # Common utilities (timeout, retry, telemetry)
│   ├── catalog.py            # Primitive registry
│   └── [implementations]     # Individual primitive files
```

All non-core primitives live in `custom_primitives/`.

Override the core root path via `ZERDA_PRIMITIVES_ROOT` environment variable.

## Primitive Sources

### Core

- `fs_read`
- `fs_write`
- `fs_replace`
- `fs_list`
- `fs_move`
- `fs_delete`
- `process_run`
- `process_spawn`
- `process_poll`
- `process_terminate`
- `shell`

### Custom

- `agent_browser`
- `extract_main_text_from_html`
- `firecrawl_search_web`
- `smart_search`
- `scrapling_fetch_page`

## Runtime Discovery

Prompt-native discovery helpers:

- `<PTC_AVALIABLE_PRIMITIVES>`
- `help(...)`

Prompt policy:

- `<PTC_AVALIABLE_PRIMITIVES>` lists only top-level public primitive names from both core and custom sources.
- Core filesystem and process primitives such as `fs_read` are included in that block just like custom primitives.
- Complex families may expose one top-level namespace name such as `agent_browser`; method discovery then happens through `help("agent_browser")`.
- Parameter shapes, defaults, and output fields should be learned through `help("name")`, not guessed from prompt text.
- The built-in `shell` primitive uses `command` as its canonical parameter and also accepts `cmd` as a compatibility alias for model-generated code.
- Some tools may also expose `get_workflow` for end-to-end setup or installation guidance after `help(...)` reveals that it exists.
- Current web routing is Firecrawl for URL discovery, `smart_search` for answer-style retrieval against a configured OpenAI-compatible `chat/completions` endpoint, Scrapling primitives for page fetching, and `agent_browser` for interactive validation.
- `scrapling_fetch_page` automatically routes `mp.weixin.qq.com` article URLs to a WeChat-specific extractor and `x.com` / `twitter.com` URLs to the stealth fetch path, where tweet-body extraction is preferred over full-page text.
- `scrapling_fetch_page` also retries with stealth fetch on selected dynamic-content domains when the first static result looks like a shell page or lacks usable content.
- `scrapling_fetch_page` requires `scrapling[fetchers]` and `playwright` in the unified PTC Python runtime and returns `dependency_missing` when those runtime dependencies are absent.
- `scrapling_fetch_page` may also require Playwright browser binaries when its internal stealth path is needed for selected dynamic-content domains; `run_zerda_with_deps.sh` installs Chromium before Zerda startup when `playwright` is declared in custom primitive requirements.
- Python execution surface should stay thin: prefer a single `<PTC_TOOL_CALLING>` block carrying async body code directly.

## Primitive Enablement

All primitives are enabled by default.

To disable specific primitives, configure a blacklist in `zerda.toml`:

```toml
[agent]
disabled_primitives = ["firecrawl_search_web"]
```

## Standard Return Contract

All primitives return a standardized structure:

```json
{
  "status": "ok|invalid_argument|timeout|dependency_missing|upstream_error|rate_limited|internal_error",
  "data": { ... },
  "error_code": "implementation-specific error code",
  "error_message": "Human-readable description",
  "retryable": true
}
```

## Design Constraints

- All primitives are async `def` functions - must use `await`
- Deep input validation (types, ranges, whitelists)
- Hard internal timeouts (never trust model-set timeout)
- Defensive error handling
- Idempotent by design
- Silent telemetry (only writes to `telemetry.jsonl`)

## Using Primitives in PTC Jobs

Primitives are automatically available in PTC job scripts. The runtime injects environment variables:

- `PTC_PRIMITIVES_PY_ROOT`: Path to primitives directory
- `PTC_PRIMITIVES_PY_ROOTS`: JSON array of primitive package roots
- `PTC_DISABLED_PRIMITIVES`: JSON array or comma-separated primitive blacklist

The bootstrap and startup hooks handle importing and making primitives available.
