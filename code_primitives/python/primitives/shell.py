from __future__ import annotations

from typing import Any

from .base import HARD_OPERATION_TIMEOUT_SECS, invalid_argument_result
from .process_run import process_run


async def shell(
    command: str | None = None,
    cmd: str | None = None,
    cwd: str | None = None,
    env: dict[str, str] | None = None,
    timeout_secs: float | None = None,
) -> dict[str, Any]:
    """
    [What it does]
    Run one shell command through `sh -lc` and return the standard process result.

    [Args]
    command: Primary shell command string to execute.
    cmd: Compatibility alias for command. Use this only when older examples or model habits expect `cmd=...`.
    cwd: Optional working directory for the shell process.
    env: Optional environment variable overrides.
    timeout_secs: Optional execution timeout in seconds.

    [Output Contract]
    Returns the same structured payload as `process_run`, including argv, exit_code, stdout, and stderr.

    [When NOT to use]
    Do not use this when you already have a safe argv list. Use `process_run` for direct argument execution without shell parsing.

    [Common Mistakes]
    Prefer `command=` in new code. `cmd=` is accepted as a compatibility alias. Do not pass an empty command string.
    """
    value = str(command or "").strip() or str(cmd or "").strip()
    if not value:
        return invalid_argument_result(
            "One of parameter command or cmd must not be empty"
        ).to_public_dict()
    return await process_run(
        argv=["sh", "-lc", value],
        cwd=cwd,
        env=env,
        timeout_secs=timeout_secs or HARD_OPERATION_TIMEOUT_SECS,
    )
