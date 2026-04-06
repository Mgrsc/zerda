from __future__ import annotations

import shutil
from pathlib import Path
from typing import Any

from .base import invalid_argument_result, load_context, run_with_guard
from .types import ActionStatus, PrimitiveResult


def _move_operation(src: str, dst: str) -> PrimitiveResult:
    source = Path(src).expanduser()
    target = Path(dst).expanduser()
    if not source.exists():
        return PrimitiveResult(
            status=ActionStatus.UPSTREAM_ERROR,
            error_code="not_found",
            error_message=f"Source path does not exist: {source}",
            retryable=False,
        )
    target.parent.mkdir(parents=True, exist_ok=True)
    shutil.move(str(source), str(target))
    return PrimitiveResult(
        status=ActionStatus.OK,
        data={"src": str(source), "dst": str(target)},
        retryable=False,
    )


async def fs_move(src: str, dst: str) -> dict[str, Any]:
    ctx = load_context()
    source = str(src or "").strip()
    target = str(dst or "").strip()
    if not source or not target:
        return invalid_argument_result("Parameters src and dst must not be empty").to_public_dict()
    result = await run_with_guard(
        primitive_name="fs_move",
        ctx=ctx,
        operation=lambda: _move_operation(source, target),
        max_retries=0,
    )
    return result.to_public_dict()
