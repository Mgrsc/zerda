from __future__ import annotations

import asyncio
import subprocess
from typing import Any

from .base import invalid_argument_result


async def process_poll(pid: int) -> dict[str, Any]:
    try:
        parsed_pid = int(pid)
    except (TypeError, ValueError):
        return invalid_argument_result("Parameter pid must be an integer").to_public_dict()
    try:
        completed = await asyncio.to_thread(
            subprocess.run,
            ["ps", "-p", str(parsed_pid), "-o", "pid=,stat=,etime=,command="],
            text=True,
            capture_output=True,
            check=False,
        )
    except Exception as exc:
        return {
            "status": "internal_error",
            "data": None,
            "error_code": "poll_error",
            "error_message": str(exc),
            "retryable": False,
        }
    output = completed.stdout.strip()
    if completed.returncode != 0 or not output:
        return {
            "status": "upstream_error",
            "data": {"pid": parsed_pid, "running": False},
            "error_code": "not_found",
            "error_message": f"Process {parsed_pid} not found",
            "retryable": False,
        }
    return {
        "status": "ok",
        "data": {"pid": parsed_pid, "running": True, "ps": output},
        "error_code": None,
        "error_message": None,
        "retryable": False,
    }
