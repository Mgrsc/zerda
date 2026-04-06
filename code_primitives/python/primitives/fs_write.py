from __future__ import annotations

from pathlib import Path
from typing import Any

from .base import invalid_argument_result, load_context, run_with_guard
from .types import ActionStatus, PrimitiveResult


def _write_operation(path: str, content: str) -> PrimitiveResult:
    resolved = Path(path).expanduser()
    resolved.parent.mkdir(parents=True, exist_ok=True)
    resolved.write_text(content, encoding="utf-8")
    return PrimitiveResult(
        status=ActionStatus.OK,
        data={"path": str(resolved), "written_chars": len(content)},
        retryable=False,
    )


async def fs_write(path: str, content: str) -> dict[str, Any]:
    ctx = load_context()
    target = str(path or "").strip()
    if not target:
        return invalid_argument_result("Parameter path must not be empty").to_public_dict()
    payload = "" if content is None else str(content)
    result = await run_with_guard(
        primitive_name="fs_write",
        ctx=ctx,
        operation=lambda: _write_operation(target, payload),
        max_retries=0,
    )
    return result.to_public_dict()
