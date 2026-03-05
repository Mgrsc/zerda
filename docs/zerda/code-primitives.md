# Code Primitives

## Overview

Code primitives are prewritten async Python functions injected into executor scripts. They provide reliable, well-tested implementations for common operations.

## Location

Primitives are located in `code_primitives/python/`:

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

Override the root path via `ZERDA_PRIMITIVES_ROOT` environment variable.

## Available Primitives

### `extract_main_text_from_html`

Extract article text from raw HTML using standard library (no external dependencies).

- Input: HTML string
- Output: `data.markdown`, `data.html`, `data.metadata`

### `firecrawl_scrape_page`

Scrape a URL using the Firecrawl API.

- Requires: `FIRECRAWL_API_KEY` environment variable
- Input: URL string
- Output: `data.markdown`, `data.html`, `data.metadata`, `data.results`
- Features: Input validation, hard timeout, retry policy

### `firecrawl_search_web`

Web search via Firecrawl API.

- Requires: `FIRECRAWL_API_KEY` environment variable
- Input: Search query
- Output: Search results with normalized flat access

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
  "status": "ok|partial|error|timeout",
  "data": { ... },
  "error_code": "INVALID_ARGUMENT|TIMEOUT|DEPENDENCY_MISSING|UPSTREAM_ERROR|RATE_LIMITED",
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

## Using Primitives in Executor

Primitives are automatically available in executor scripts. The executor injects environment variables:

- `EXECUTOR_PRIMITIVES_PY_ROOT`: Path to primitives directory
- `EXECUTOR_DISABLED_PRIMITIVES`: JSON array or comma-separated primitive blacklist

The bootstrap and startup hooks handle importing and making primitives available.
