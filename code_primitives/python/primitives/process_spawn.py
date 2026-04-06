from __future__ import annotations

import asyncio
import os
import subprocess
from typing import Any

from .base import invalid_argument_result


def _merge_env(env: dict[str, str] | None) -> dict[str, str]:
    merged = dict(os.environ)
    if env:
        merged.update({str(k): str(v) for k, v in env.items()})
    return merged


async def process_spawn(
    argv: list[str],
    cwd: str | None = None,
    env: dict[str, str] | None = None,
) -> dict[str, Any]:
    if not isinstance(argv, list) or not argv:
        return invalid_argument_result("Parameter argv must be a non-empty list").to_public_dict()
    command = [str(item) for item in argv]
    try:
        proc = await asyncio.to_thread(
            subprocess.Popen,
            command,
            cwd=cwd or None,
            env=_merge_env(env),
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            start_new_session=True,
            text=True,
        )
    except Exception as exc:
        return {
            "status": "internal_error",
            "data": None,
            "error_code": "spawn_error",
            "error_message": str(exc),
            "retryable": False,
        }
    return {
        "status": "ok",
        "data": {"pid": proc.pid, "argv": command, "cwd": cwd},
        "error_code": None,
        "error_message": None,
        "retryable": False,
    }
