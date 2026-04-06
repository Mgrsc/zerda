# Code Primitives Specification

This directory stores core Python primitives that can be injected and called directly by PTC job scripts.

The goal is to make the model reuse stable primitives during execution, reducing temporary script stitching, parameter hallucination, and context noise.

## Dependency Policy

- Primitive implementations should prefer the Python standard library by default.
- Introduce third-party dependencies only when the standard library cannot satisfy the requirement.
- If a third-party dependency is required, it must be preinstalled in the container image and explicitly documented in this README. PTC job code must not install dependencies on the fly during a task.
- Missing dependencies must return `DEPENDENCY_MISSING` instead of crashing the primitive.

## Directory Structure

- `python/bootstrap.py`
  - Runtime injection entry point that registers primitives into the script global namespace
- `python/sitecustomize.py`
  - Python startup hook that injects primitives into `builtins` for direct access in any Python session
- `python/primitives/types.py`
  - Shared types: `ActionStatus` and `PrimitiveResult`
- `python/primitives/base.py`
  - Shared capabilities: hard timeout, retry, input validation, telemetry persistence, Firecrawl HTTP wrapper
- `python/primitives/catalog.py`
  - Core primitive registry
- `python/primitives/*.py`
  - Core primitive implementation files
- `../custom_primitives/*.py`
  - Non-core primitive implementation files, including bundled integrations and user-added primitives

## Hard Constraints for Primitive Design

### 1. Strong Typing and Structured Return

Every primitive must return the `PrimitiveResult.to_public_dict()` structure:

- `status`
- `data`
- `error_code`
- `error_message`
- `retryable`

Status codes must use `ActionStatus`. Do not return arbitrary status strings.

All injected primitives must be declared as `async def`. Callers must use `await` and must not treat them as synchronous functions.

### 2. Mandatory Hard Timeout

Any network I/O or heavy computation must use internal hard timeout constants. Do not trust timeout values supplied by the model.

Timeout failures must return:

- `status = ActionStatus.TIMEOUT`
- `error_code = "operation_timeout"` or `"network_timeout"`

### 3. Deep Input Validation

Type annotations alone are not enough. Boundary and format validation must be performed inside each primitive.

Examples:

- URL must be `http/https`
- `limit` must stay within the defined range
- `sources/formats` must be in allowlists

Validation failure must return:

- `status = ActionStatus.INVALID_ARGUMENT`
- A clear `error_message` that can guide model self-correction on the next iteration

### 4. Defensive Error Handling

Primitives must not rethrow low-level exceptions and crash the upper execution flow.

Requirements:

- Catch missing dependency, network error, and upstream HTTP error
- Convert to standard status codes (`DEPENDENCY_MISSING`, `UPSTREAM_ERROR`, `RATE_LIMITED`, etc.)
- Mark whether the result is retryable (`retryable`)

### 5. Idempotency First

Primitives should be idempotent by default. Repeating the same call with identical parameters should not pollute state.

Avoid:

- Append-style file writes
- Unnecessary side effects

### 6. Silent Telemetry and Result Isolation

Telemetry must be written only to `telemetry.jsonl` inside task artifacts and must not be directly injected into Planner context.

Minimum telemetry fields:

- primitive name
- status code
- duration
- retry count
- key error code

## Docstring Specification (LLM-Oriented)

Each public primitive must contain these sections:

- `[What it does]`
- `[Args]`
- `[Returns]`
- `[Output Contract]`
- `[When NOT to use]`
- `[Common Mistakes]`

Important: `[When NOT to use]` must clearly explain misuse cases to reduce wrong primitive selection by the model.

`[Output Contract]` must explicitly define the success condition and key field paths so PTC job code can read them strictly by contract instead of guessing key names.

## Naming Convention

- Primitive function names must be precise and semantically complete. Avoid abbreviations and vague names.
- Use verb + object naming, for example:
  - `firecrawl_search_web`
- Do not use natural-language names such as "web page crawling" as function identifiers.

## Environment Variable Gating

The Firecrawl search primitive is available only when configuration is present:

- `FIRECRAWL_API_KEY` (primary)
- `FIRECRAWL_KEY` (compatibility fallback)
- `FIRECRAWL_BASE_URL` (optional)

If the key is missing, the primitive must return `DEPENDENCY_MISSING`, and upper layers should avoid aggressive retry loops.

Primitive enablement uses a blacklist. All primitives are enabled by default; disable selected names via `agent.disabled_primitives` in `zerda.toml`.

## Steps to Add a New Primitive

1. Choose the correct source:
   - core: `python/primitives/`
   - non-core custom: `../custom_primitives/`
2. Implement an async function with validation, hard timeout, retry, docstring, and structured return according to this spec.
3. Register the function in the matching `catalog.py`.
4. Ensure function signature and docstring can be parsed by the scanner (they are injected into the runtime primitive catalog).

## Core Built-in Primitives

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

## Custom Primitives

- `extract_main_text_from_html`
  - Standard-library primitive that extracts readable main text and title from HTML
- `firecrawl_search_web`
  - Requires `FIRECRAWL_API_KEY`
- `scrapling_fetch_page`
  - Requires `scrapling[fetchers]` in the Python runtime
  - Default page-fetch primitive with built-in routing for WeChat articles, X/Twitter, and selected dynamic-site stealth fallback

## Primitive Template

```python
from __future__ import annotations

from typing import Any

from .base import invalid_argument_result, load_context, run_with_guard
from .types import PrimitiveResult


def _operation(...) -> PrimitiveResult:
    ...


async def your_primitive(...) -> dict[str, Any]:
    """
    [What it does]
    ...

    [Args]
    ...

    [Returns]
    PrimitiveResult public dict: status/data/error_code/error_message/retryable

    [When NOT to use]
    ...

    [Common Mistakes]
    ...
    """
    ctx = load_context()
    try:
        ...
    except ValueError as exc:
        return invalid_argument_result(str(exc)).to_public_dict()

    result = await run_with_guard(
        primitive_name="your_primitive",
        ctx=ctx,
        operation=lambda: _operation(...),
    )
    return result.to_public_dict()
```
