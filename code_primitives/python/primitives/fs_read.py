from __future__ import annotations

from pathlib import Path
from typing import Any

from .base import invalid_argument_result, load_context, run_with_guard
from .types import ActionStatus, PrimitiveResult

MAX_READ_CHARS = 2_000_000


def _read_operation(path: str) -> PrimitiveResult:
    resolved = Path(path).expanduser()
    if not resolved.exists():
        return PrimitiveResult(
            status=ActionStatus.UPSTREAM_ERROR,
            error_code="not_found",
            error_message=f"Path does not exist: {resolved}",
            retryable=False,
        )
    if not resolved.is_file():
        return PrimitiveResult(
            status=ActionStatus.INVALID_ARGUMENT,
            error_code="not_a_file",
            error_message=f"Path is not a file: {resolved}",
            retryable=False,
        )
    content = resolved.read_text(encoding="utf-8", errors="replace")
    if len(content) > MAX_READ_CHARS:
        content = content[:MAX_READ_CHARS]
    return PrimitiveResult(
        status=ActionStatus.OK,
        data={"path": str(resolved), "content": content},
        retryable=False,
    )


async def fs_read(path: str) -> dict[str, Any]:
    ctx = load_context()
    value = str(path or "").strip()
    if not value:
        return invalid_argument_result("Parameter path must not be empty").to_public_dict()
    result = await run_with_guard(
        primitive_name="fs_read",
        ctx=ctx,
        operation=lambda: _read_operation(value),
        max_retries=0,
    )
    return result.to_public_dict()
