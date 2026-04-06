from __future__ import annotations

import hashlib
import json
import os
from datetime import datetime, timezone
from pathlib import Path
import re
import shutil
import subprocess
from typing import Any, Sequence
from urllib.parse import urlparse

from primitives.base import (
    HARD_OPERATION_TIMEOUT_SECS,
    dependency_missing_result,
)
from primitives.types import ActionStatus, PrimitiveResult

MAX_SESSION_LENGTH = 120
MAX_TEXT_LENGTH = 10_000
MAX_SELECTOR_LENGTH = 2_048
MAX_JS_LENGTH = 8_000
MAX_ARTIFACT_PATH_LENGTH = 240
MAX_ATTR_NAME_LENGTH = 128
MAX_KEY_LENGTH = 128
MAX_DIRECTION_LENGTH = 32
MAX_TIMEOUT_MS = 120_000
MAX_SNAPSHOT_DEPTH = 20
AGENT_BROWSER_TIMEOUT_SECS = min(HARD_OPERATION_TIMEOUT_SECS, 20.0)
SESSION_PATTERN = re.compile(r"^[A-Za-z0-9._-]{1,120}$")
REF_PATTERN = re.compile(r"@e\d+")
SUPPORTED_GET_KINDS = ("text", "html", "value", "attr", "title", "url")


def validate_session(raw: Any) -> str | None:
    if raw is None:
        return None
    value = str(raw).strip()
    if not value:
        return None
    if len(value) > MAX_SESSION_LENGTH:
        raise ValueError(f"Parameter session exceeds max length {MAX_SESSION_LENGTH}")
    if not SESSION_PATTERN.fullmatch(value):
        raise ValueError(
            "Parameter session must contain only letters, numbers, dot, underscore, or dash"
        )
    return value


def validate_short_text(
    raw: Any,
    field_name: str,
    *,
    max_length: int = MAX_TEXT_LENGTH,
) -> str:
    value = str(raw or "").strip()
    if not value:
        raise ValueError(f"Parameter {field_name} must not be empty")
    if len(value) > max_length:
        raise ValueError(f"Parameter {field_name} exceeds max length {max_length}")
    return value


def validate_selector(raw: Any, field_name: str = "target") -> str:
    return validate_short_text(raw, field_name, max_length=MAX_SELECTOR_LENGTH)


def validate_cdp_target(raw: Any) -> str:
    value = validate_short_text(raw, "target", max_length=MAX_SELECTOR_LENGTH)
    if value.isdigit():
        port = int(value)
        if port < 1 or port > 65535:
            raise ValueError("Parameter target port must be between 1 and 65535")
        return value
    parsed = urlparse(value)
    if parsed.scheme not in {"ws", "wss", "http", "https"} or not parsed.netloc:
        raise ValueError(
            "Parameter target must be a port number or a ws/wss/http/https CDP endpoint"
        )
    return value


def validate_artifact_path(raw: Any, default_name: str) -> str:
    artifact_root = Path(os.environ.get("PTC_ARTIFACT_DIR", ".")).expanduser().resolve()
    if raw is None or not str(raw).strip():
        candidate = artifact_root / default_name
    else:
        value = str(raw).strip()
        if len(value) > MAX_ARTIFACT_PATH_LENGTH:
            raise ValueError(
                f"Parameter path exceeds max length {MAX_ARTIFACT_PATH_LENGTH}"
            )
        candidate = Path(value)
        if candidate.is_absolute():
            raise ValueError("Parameter path must be relative to the task artifact directory")
        candidate = (artifact_root / candidate).resolve()
    if not candidate.is_relative_to(artifact_root):
        raise ValueError("Parameter path must stay within the task artifact directory")
    candidate.parent.mkdir(parents=True, exist_ok=True)
    return str(candidate)


def current_ptc_session_key() -> str | None:
    value = os.environ.get("PTC_SESSION_KEY", "").strip()
    return value or None


def _browser_state_root() -> Path:
    return Path.home().expanduser().resolve() / ".zerda" / "agent_browser_state"


def browser_state_path(session_key: str | None = None) -> Path | None:
    scoped_key = session_key or current_ptc_session_key()
    if not scoped_key:
        return None
    digest = hashlib.sha256(scoped_key.encode("utf-8")).hexdigest()
    return _browser_state_root() / f"{digest}.json"


def load_browser_state(session_key: str | None = None) -> dict[str, Any]:
    path = browser_state_path(session_key)
    if path is None or not path.exists():
        return {}
    try:
        parsed = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return {}
    return parsed if isinstance(parsed, dict) else {}


def save_browser_state(state: dict[str, Any], session_key: str | None = None) -> bool:
    path = browser_state_path(session_key)
    if path is None:
        return False
    try:
        path.parent.mkdir(parents=True, exist_ok=True)
        tmp_path = path.with_suffix(".tmp")
        payload = json.dumps(state, ensure_ascii=False, indent=2)
        tmp_path.write_text(payload, encoding="utf-8")
        tmp_path.replace(path)
        return True
    except OSError:
        return False


def clear_browser_state(session_key: str | None = None) -> bool:
    path = browser_state_path(session_key)
    if path is None or not path.exists():
        return False
    try:
        path.unlink()
        return True
    except OSError:
        return False


def derive_default_browser_session(session_key: str | None = None) -> str | None:
    scoped_key = session_key or current_ptc_session_key()
    if not scoped_key:
        return None
    digest = hashlib.sha256(scoped_key.encode("utf-8")).hexdigest()[:24]
    return f"zerda-{digest}"


def utc_now_iso() -> str:
    return datetime.now(timezone.utc).isoformat()


def build_agent_browser_command(
    command_argv: Sequence[str],
    *,
    session: str | None = None,
    headed: bool = False,
) -> list[str]:
    argv = ["--json"]
    if session:
        argv.extend(["--session", session])
    if headed:
        argv.append("--headed")
    argv.extend(command_argv)
    return argv


def parse_agent_browser_output(stdout: str) -> Any:
    text = stdout.strip()
    if not text:
        return None
    try:
        return json.loads(text)
    except json.JSONDecodeError:
        return text


def normalize_command_data(
    command: Sequence[str],
    session: str | None,
    stdout: str,
    stderr: str,
    parsed: Any,
) -> dict[str, Any]:
    return {
        "command": list(command),
        "session": session,
        "stdout_raw": stdout,
        "stderr_raw": stderr,
        "parsed": parsed,
    }


def extract_first(parsed: Any, *paths: Sequence[str]) -> Any:
    for path in paths:
        current = parsed
        matched = True
        for part in path:
            if not isinstance(current, dict) or part not in current:
                matched = False
                break
            current = current[part]
        if matched and current is not None:
            return current
    return None


def extract_text_payload(data: dict[str, Any]) -> str:
    parsed = data.get("parsed")
    value = extract_first(
        parsed,
        ("snapshot",),
        ("text",),
        ("content",),
        ("output",),
        ("data", "snapshot"),
        ("data", "text"),
        ("result",),
    )
    if isinstance(value, str) and value.strip():
        return value
    if isinstance(parsed, str) and parsed.strip():
        return parsed
    stdout = str(data.get("stdout_raw") or "").strip()
    if stdout:
        return stdout
    return ""


def extract_refs(text: str) -> list[str]:
    refs = REF_PATTERN.findall(text)
    return sorted(set(refs), key=refs.index)


def infer_value_from_parsed(parsed: Any) -> Any:
    if isinstance(parsed, dict):
        value = extract_first(
            parsed,
            ("value",),
            ("text",),
            ("html",),
            ("title",),
            ("url",),
            ("cdpUrl",),
            ("attribute",),
            ("attr",),
            ("count",),
            ("data", "value"),
            ("data", "text"),
            ("data", "html"),
            ("data", "title"),
            ("data", "url"),
            ("data", "cdpUrl"),
            ("data", "count"),
        )
        if value is not None:
            return value
    if isinstance(parsed, (str, int, float, bool)):
        return parsed
    return None


def run_agent_browser(
    command_argv: Sequence[str],
    *,
    session: str | None = None,
    headed: bool = False,
    timeout_secs: float = AGENT_BROWSER_TIMEOUT_SECS,
) -> PrimitiveResult:
    binary = os.environ.get("AGENT_BROWSER_EXECUTABLE_PATH", "").strip() or shutil.which(
        "agent-browser"
    )
    if not binary:
        return dependency_missing_result(
            "Missing agent-browser on PATH; install the CLI before using browser primitives"
        )

    command = build_agent_browser_command(command_argv, session=session, headed=headed)
    argv = [binary, *command]
    try:
        completed = subprocess.run(
            argv,
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=timeout_secs,
            cwd=os.environ.get("PTC_WORKING_DIR") or None,
        )
    except subprocess.TimeoutExpired:
        return PrimitiveResult(
            status=ActionStatus.TIMEOUT,
            error_code="agent_browser_timeout",
            error_message=f"agent-browser exceeded timeout {timeout_secs}s",
            retryable=True,
        )
    except OSError as exc:
        return dependency_missing_result(
            f"Failed to start agent-browser: {exc}"
        )

    stdout = completed.stdout or ""
    stderr = completed.stderr or ""
    parsed = parse_agent_browser_output(stdout)
    data = normalize_command_data(command, session, stdout, stderr, parsed)
    if completed.returncode == 0:
        return PrimitiveResult(
            status=ActionStatus.OK,
            data=data,
            retryable=False,
        )

    combined = "\n".join(part for part in [stderr.strip(), stdout.strip()] if part).strip()
    lowered = combined.lower()
    if "agent-browser install" in lowered or (
        "chrome" in lowered and ("not found" in lowered or "install" in lowered)
    ):
        missing = dependency_missing_result(
            f"agent-browser runtime dependency is missing or not installed correctly: {combined}"
        )
        missing.data = data
        return missing

    return PrimitiveResult(
        status=ActionStatus.UPSTREAM_ERROR,
        data=data,
        error_code="agent_browser_command_failed",
        error_message=f"agent-browser command failed with exit code {completed.returncode}: {combined}",
        retryable=False,
    )
