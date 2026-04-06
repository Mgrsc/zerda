from __future__ import annotations

from pathlib import Path
from typing import Any

from .base import invalid_argument_result, load_context, run_with_guard, validate_int_range
from .types import ActionStatus, PrimitiveResult


def _list_operation(path: str, recursive: bool, max_entries: int) -> PrimitiveResult:
    resolved = Path(path).expanduser()
    if not resolved.exists() or not resolved.is_dir():
        return PrimitiveResult(
            status=ActionStatus.UPSTREAM_ERROR,
            error_code="not_found",
            error_message=f"Path is not a directory: {resolved}",
            retryable=False,
        )
    iterator = resolved.rglob("*") if recursive else resolved.iterdir()
    entries: list[dict[str, Any]] = []
    for entry in iterator:
        entries.append(
            {
                "path": str(entry),
                "name": entry.name,
                "kind": "dir" if entry.is_dir() else "file",
            }
        )
        if len(entries) >= max_entries:
            break
    return PrimitiveResult(
        status=ActionStatus.OK,
        data={"path": str(resolved), "entries": entries, "count": len(entries)},
        retryable=False,
    )


async def fs_list(
    path: str,
    recursive: bool = False,
    max_entries: int = 200,
) -> dict[str, Any]:
    ctx = load_context()
    target = str(path or "").strip()
    if not target:
        return invalid_argument_result("Parameter path must not be empty").to_public_dict()
    try:
        parsed_max_entries = validate_int_range(max_entries, "max_entries", 1, 5000)
    except ValueError as exc:
        return invalid_argument_result(str(exc)).to_public_dict()
    result = await run_with_guard(
        primitive_name="fs_list",
        ctx=ctx,
        operation=lambda: _list_operation(target, bool(recursive), parsed_max_entries),
        max_retries=0,
    )
    return result.to_public_dict()
