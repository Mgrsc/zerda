from __future__ import annotations

import shutil
from pathlib import Path
from typing import Any

from .base import invalid_argument_result, load_context, run_with_guard
from .types import ActionStatus, PrimitiveResult


def _delete_operation(path: str, recursive: bool) -> PrimitiveResult:
    resolved = Path(path).expanduser()
    if not resolved.exists():
        return PrimitiveResult(
            status=ActionStatus.UPSTREAM_ERROR,
            error_code="not_found",
            error_message=f"Path does not exist: {resolved}",
            retryable=False,
        )
    if resolved.is_dir():
        if not recursive:
            return PrimitiveResult(
                status=ActionStatus.INVALID_ARGUMENT,
                error_code="recursive_required",
                error_message="Directory deletion requires recursive=True",
                retryable=False,
            )
        shutil.rmtree(resolved)
    else:
        resolved.unlink()
    return PrimitiveResult(
        status=ActionStatus.OK,
        data={"path": str(resolved)},
        retryable=False,
    )


async def fs_delete(path: str, recursive: bool = False) -> dict[str, Any]:
    ctx = load_context()
    target = str(path or "").strip()
    if not target:
        return invalid_argument_result("Parameter path must not be empty").to_public_dict()
    result = await run_with_guard(
        primitive_name="fs_delete",
        ctx=ctx,
        operation=lambda: _delete_operation(target, bool(recursive)),
        max_retries=0,
    )
    return result.to_public_dict()
