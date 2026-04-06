from __future__ import annotations

from pathlib import Path
from typing import Any

from .base import invalid_argument_result, load_context, run_with_guard
from .types import ActionStatus, PrimitiveResult


def _replace_operation(path: str, old: str, new: str, count: int | None) -> PrimitiveResult:
    resolved = Path(path).expanduser()
    if not resolved.exists() or not resolved.is_file():
        return PrimitiveResult(
            status=ActionStatus.UPSTREAM_ERROR,
            error_code="not_found",
            error_message=f"Path is not a file: {resolved}",
            retryable=False,
        )
    original = resolved.read_text(encoding="utf-8", errors="replace")
    matches = original.count(old)
    replaced = (
        original.replace(old, new)
        if count is None
        else original.replace(old, new, count)
    )
    if replaced == original:
        return PrimitiveResult(
            status=ActionStatus.UPSTREAM_ERROR,
            error_code="no_match",
            error_message="No matching content found to replace",
            retryable=False,
        )
    resolved.write_text(replaced, encoding="utf-8")
    applied = matches if count is None else min(matches, count)
    return PrimitiveResult(
        status=ActionStatus.OK,
        data={
            "path": str(resolved),
            "replacements_applied": applied,
        },
        retryable=False,
    )


async def fs_replace(
    path: str,
    old: str,
    new: str,
    count: int | None = None,
) -> dict[str, Any]:
    ctx = load_context()
    target = str(path or "").strip()
    before = str(old or "")
    after = "" if new is None else str(new)
    if not target:
        return invalid_argument_result("Parameter path must not be empty").to_public_dict()
    if not before:
        return invalid_argument_result("Parameter old must not be empty").to_public_dict()
    parsed_count = None if count is None else int(count)
    result = await run_with_guard(
        primitive_name="fs_replace",
        ctx=ctx,
        operation=lambda: _replace_operation(target, before, after, parsed_count),
        max_retries=0,
    )
    return result.to_public_dict()
