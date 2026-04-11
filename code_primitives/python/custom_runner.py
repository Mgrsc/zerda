from __future__ import annotations

import asyncio
import importlib
import json
import os
import sys
import traceback
from pathlib import Path


def _resolve_roots() -> list[Path]:
    raw_many = os.environ.get("PTC_PRIMITIVES_PY_ROOTS", "").strip()
    if raw_many:
        try:
            parsed = json.loads(raw_many)
            if isinstance(parsed, list):
                roots = [
                    Path(str(item)).expanduser().resolve()
                    for item in parsed
                    if str(item).strip()
                ]
                if roots:
                    return roots
        except json.JSONDecodeError:
            pass

    roots: list[Path] = []
    root = os.environ.get("PTC_PRIMITIVES_PY_ROOT", "").strip()
    if root:
        roots.append(Path(root).expanduser().resolve())
    working_dir = os.environ.get("PTC_WORKING_DIR", "").strip()
    if working_dir:
        roots.append(Path(working_dir).expanduser().resolve())
    return roots


for root in _resolve_roots():
    resolved = str(root)
    if resolved not in sys.path:
        sys.path.insert(0, resolved)


def _error(status: str, error_code: str, error_message: str, *, retryable: bool) -> dict:
    return {
        "status": status,
        "data": {},
        "error_code": error_code,
        "error_message": error_message,
        "retryable": retryable,
    }


async def _invoke() -> dict:
    if len(sys.argv) != 4:
        return _error(
            "internal_error",
            "invoke_failed.invalid_runner_args",
            "custom runner requires module, callable, and argument payload",
            retryable=False,
        )

    module_name, callable_name, raw_payload = sys.argv[1], sys.argv[2], sys.argv[3]
    try:
        payload = json.loads(raw_payload)
    except json.JSONDecodeError as exc:
        return _error(
            "invalid_argument",
            "invoke_failed.invalid_payload",
            f"failed to decode custom primitive payload: {exc}",
            retryable=False,
        )
    if not isinstance(payload, dict):
        return _error(
            "invalid_argument",
            "invoke_failed.invalid_payload",
            "custom primitive payload must be a JSON object",
            retryable=False,
        )

    if "args" in payload or "kwargs" in payload:
        args = payload.get("args", [])
        kwargs = payload.get("kwargs", {})
        if not isinstance(args, list):
            return _error(
                "invalid_argument",
                "invoke_failed.invalid_payload",
                "custom primitive positional arguments must be a JSON array",
                retryable=False,
            )
        if not isinstance(kwargs, dict):
            return _error(
                "invalid_argument",
                "invoke_failed.invalid_payload",
                "custom primitive keyword arguments must be a JSON object",
                retryable=False,
            )
    else:
        args = []
        kwargs = payload

    try:
        module = importlib.import_module(module_name)
        fn = getattr(module, callable_name)
        return await fn(*args, **kwargs)
    except ModuleNotFoundError as exc:
        return _error(
            "dependency_missing",
            "not_ready.module_import",
            f"failed to import custom primitive dependency: {exc}",
            retryable=False,
        )
    except Exception as exc:
        traceback.print_exc(file=sys.stderr)
        return _error(
            "internal_error",
            "primitive_error.uncaught_exception",
            f"custom primitive execution failed: {exc}",
            retryable=False,
        )


if __name__ == "__main__":
    result = asyncio.run(_invoke())
    print(json.dumps(result, ensure_ascii=False))
