from __future__ import annotations

import asyncio
import os
import subprocess
from typing import Any

from .base import HARD_OPERATION_TIMEOUT_SECS, invalid_argument_result


def _merge_env(env: dict[str, str] | None) -> dict[str, str]:
    merged = dict(os.environ)
    if env:
        merged.update({str(k): str(v) for k, v in env.items()})
    return merged


async def process_run(
    argv: list[str],
    cwd: str | None = None,
    env: dict[str, str] | None = None,
    timeout_secs: float | None = None,
) -> dict[str, Any]:
    if not isinstance(argv, list) or not argv:
        return invalid_argument_result("Parameter argv must be a non-empty list").to_public_dict()
    command = [str(item) for item in argv]
    effective_timeout = float(timeout_secs or HARD_OPERATION_TIMEOUT_SECS)
    try:
        completed = await asyncio.to_thread(
            subprocess.run,
            command,
            cwd=cwd or None,
            env=_merge_env(env),
            text=True,
            capture_output=True,
            timeout=effective_timeout,
            check=False,
        )
    except subprocess.TimeoutExpired:
        return {
            "status": "timeout",
            "data": None,
            "error_code": "process_timeout",
            "error_message": f"Process exceeded timeout {effective_timeout}s",
            "retryable": True,
        }
    except Exception as exc:
        return {
            "status": "internal_error",
            "data": None,
            "error_code": "process_error",
            "error_message": str(exc),
            "retryable": False,
        }
    return {
        "status": "ok" if completed.returncode == 0 else "upstream_error",
        "data": {
            "argv": command,
            "cwd": cwd,
            "exit_code": completed.returncode,
            "stdout": completed.stdout,
            "stderr": completed.stderr,
        },
        "error_code": None if completed.returncode == 0 else "non_zero_exit",
        "error_message": None if completed.returncode == 0 else f"Process exited with code {completed.returncode}",
        "retryable": False,
    }
