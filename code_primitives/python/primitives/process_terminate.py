from __future__ import annotations

import asyncio
import subprocess
from typing import Any

from .base import invalid_argument_result


async def process_terminate(pid: int, signal: str = "TERM") -> dict[str, Any]:
    try:
        parsed_pid = int(pid)
    except (TypeError, ValueError):
        return invalid_argument_result("Parameter pid must be an integer").to_public_dict()
    sig = str(signal or "TERM").strip().upper()
    if not sig:
        sig = "TERM"
    try:
        completed = await asyncio.to_thread(
            subprocess.run,
            ["kill", f"-{sig}", str(parsed_pid)],
            text=True,
            capture_output=True,
            check=False,
        )
    except Exception as exc:
        return {
            "status": "internal_error",
            "data": None,
            "error_code": "terminate_error",
            "error_message": str(exc),
            "retryable": False,
        }
    return {
        "status": "ok" if completed.returncode == 0 else "upstream_error",
        "data": {"pid": parsed_pid, "signal": sig},
        "error_code": None if completed.returncode == 0 else "kill_failed",
        "error_message": None if completed.returncode == 0 else completed.stderr.strip() or f"kill exited with {completed.returncode}",
        "retryable": False,
    }
