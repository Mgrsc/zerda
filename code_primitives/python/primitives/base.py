from __future__ import annotations

import asyncio
import json
import os
import re
import socket
import time
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Callable
from urllib.error import HTTPError, URLError
from urllib.parse import urlparse
from urllib.request import Request, urlopen

from .types import ActionStatus, PrimitiveResult

HARD_NETWORK_TIMEOUT_SECS = 15.0
HARD_OPERATION_TIMEOUT_SECS = 25.0
MAX_RETRIES = 2
BACKOFF_BASE_SECS = 0.6
MAX_URL_LENGTH = 2048
URL_PATTERN = re.compile(r"^https?://[^\s]+$", re.IGNORECASE)
DEFAULT_FIRECRAWL_BASE_URL = "https://api.firecrawl.dev"


@dataclass(slots=True)
class PrimitiveContext:
    telemetry_path: Path
    firecrawl_api_key: str | None
    firecrawl_base_url: str


def load_context() -> PrimitiveContext:
    telemetry_raw = os.environ.get("EXECUTOR_TELEMETRY_PATH", "").strip()
    telemetry_path = Path(telemetry_raw) if telemetry_raw else Path("telemetry.jsonl")
    key = (
        os.environ.get("FIRECRAWL_API_KEY", "").strip()
        or os.environ.get("FIRECRAWL_KEY", "").strip()
        or None
    )
    base = os.environ.get("FIRECRAWL_BASE_URL", "").strip() or DEFAULT_FIRECRAWL_BASE_URL
    return PrimitiveContext(
        telemetry_path=telemetry_path,
        firecrawl_api_key=key,
        firecrawl_base_url=base.rstrip("/"),
    )


def _utc_now() -> str:
    return datetime.now(timezone.utc).isoformat()


def emit_telemetry(
    ctx: PrimitiveContext,
    primitive: str,
    result: PrimitiveResult,
    duration_ms: int,
    attempts: int,
) -> None:
    payload = {
        "ts": _utc_now(),
        "primitive": primitive,
        "status": result.status.value,
        "retryable": result.retryable,
        "error_code": result.error_code,
        "duration_ms": duration_ms,
        "attempts": attempts,
    }
    if result.error_message:
        payload["error_message"] = result.error_message
    if result.telemetry:
        payload["details"] = result.telemetry
    ctx.telemetry_path.parent.mkdir(parents=True, exist_ok=True)
    with ctx.telemetry_path.open("a", encoding="utf-8") as handle:
        handle.write(json.dumps(payload, ensure_ascii=False) + "\n")


def validate_http_url(raw: str, field_name: str = "url") -> str:
    value = str(raw or "").strip()
    if not value:
        raise ValueError(f"Parameter {field_name} must not be empty")
    if len(value) > MAX_URL_LENGTH:
        raise ValueError(
            f"Parameter {field_name} exceeds max length {MAX_URL_LENGTH}"
        )
    if not URL_PATTERN.match(value):
        raise ValueError(
            f"Parameter {field_name} must start with http:// or https://"
        )
    parsed = urlparse(value)
    if parsed.scheme not in ("http", "https") or not parsed.netloc:
        raise ValueError(f"Parameter {field_name} is not a valid URL")
    return value


def validate_int_range(raw: Any, field_name: str, min_value: int, max_value: int) -> int:
    if isinstance(raw, bool):
        raise ValueError(f"Parameter {field_name} must be an integer")
    try:
        value = int(raw)
    except (TypeError, ValueError):
        raise ValueError(f"Parameter {field_name} must be an integer") from None
    if value < min_value or value > max_value:
        raise ValueError(
            f"Parameter {field_name} must be between {min_value} and {max_value}"
        )
    return value


def dependency_missing_result(message: str) -> PrimitiveResult:
    return PrimitiveResult(
        status=ActionStatus.DEPENDENCY_MISSING,
        error_code="missing_dependency",
        error_message=message,
        retryable=False,
    )


def invalid_argument_result(message: str) -> PrimitiveResult:
    return PrimitiveResult(
        status=ActionStatus.INVALID_ARGUMENT,
        error_code="invalid_argument",
        error_message=message,
        retryable=False,
    )


def parse_json_bytes(raw: bytes) -> Any:
    if not raw:
        return {}
    try:
        return json.loads(raw.decode("utf-8", errors="replace"))
    except json.JSONDecodeError:
        return {"raw": raw.decode("utf-8", errors="replace")}


def firecrawl_post(
    ctx: PrimitiveContext,
    endpoint: str,
    payload: dict[str, Any],
    timeout_secs: float,
) -> PrimitiveResult:
    if not ctx.firecrawl_api_key:
        return dependency_missing_result(
            "Missing FIRECRAWL_API_KEY; cannot call firecrawl primitives"
        )
    url = f"{ctx.firecrawl_base_url}{endpoint}"
    body = json.dumps(payload, ensure_ascii=False).encode("utf-8")
    request = Request(url=url, data=body, method="POST")
    request.add_header("Authorization", f"Bearer {ctx.firecrawl_api_key}")
    request.add_header("Content-Type", "application/json")
    request.add_header("Accept", "application/json")
    try:
        with urlopen(request, timeout=timeout_secs) as response:
            raw = response.read()
            parsed = parse_json_bytes(raw)
            return PrimitiveResult(
                status=ActionStatus.OK,
                data={
                    "http_status": int(getattr(response, "status", 200)),
                    "result": parsed,
                },
                retryable=False,
            )
    except HTTPError as exc:
        raw = exc.read() if hasattr(exc, "read") else b""
        parsed = parse_json_bytes(raw)
        status_code = int(getattr(exc, "code", 0) or 0)
        if status_code == 429:
            return PrimitiveResult(
                status=ActionStatus.RATE_LIMITED,
                error_code="rate_limited",
                error_message=f"Firecrawl returned 429: {parsed}",
                retryable=True,
                telemetry={"http_status": status_code},
            )
        retryable = status_code >= 500
        return PrimitiveResult(
            status=ActionStatus.UPSTREAM_ERROR,
            error_code="upstream_http_error",
            error_message=f"Firecrawl returned {status_code}: {parsed}",
            retryable=retryable,
            telemetry={"http_status": status_code},
        )
    except URLError as exc:
        reason = str(exc.reason) if hasattr(exc, "reason") else str(exc)
        timed_out = "timed out" in reason.lower()
        return PrimitiveResult(
            status=ActionStatus.TIMEOUT if timed_out else ActionStatus.UPSTREAM_ERROR,
            error_code="network_timeout" if timed_out else "network_error",
            error_message=f"Network request failed: {reason}",
            retryable=True,
        )
    except socket.timeout:
        return PrimitiveResult(
            status=ActionStatus.TIMEOUT,
            error_code="network_timeout",
            error_message="Network request timed out",
            retryable=True,
        )


async def run_with_guard(
    primitive_name: str,
    ctx: PrimitiveContext,
    operation: Callable[[], PrimitiveResult],
    max_retries: int = MAX_RETRIES,
    hard_timeout_secs: float = HARD_OPERATION_TIMEOUT_SECS,
    backoff_base_secs: float = BACKOFF_BASE_SECS,
) -> PrimitiveResult:
    started = time.perf_counter()
    attempts = 0
    last = PrimitiveResult(
        status=ActionStatus.INTERNAL_ERROR,
        error_code="uninitialized",
        error_message="primitive did not run",
        retryable=False,
    )
    for attempt in range(max_retries + 1):
        attempts = attempt + 1
        try:
            result = await asyncio.wait_for(
                asyncio.to_thread(operation),
                timeout=hard_timeout_secs,
            )
        except asyncio.TimeoutError:
            result = PrimitiveResult(
                status=ActionStatus.TIMEOUT,
                error_code="operation_timeout",
                error_message=(
                    f"Primitive execution exceeded hard timeout {hard_timeout_secs}s"
                ),
                retryable=True,
            )
        except Exception as exc:
            result = PrimitiveResult(
                status=ActionStatus.INTERNAL_ERROR,
                error_code="internal_error",
                error_message=f"Primitive internal error: {exc}",
                retryable=False,
            )
        last = result
        if result.status == ActionStatus.OK:
            break
        if not result.retryable:
            break
        if attempt < max_retries:
            await asyncio.sleep(backoff_base_secs * (2**attempt))
    duration_ms = int((time.perf_counter() - started) * 1000)
    emit_telemetry(ctx, primitive_name, last, duration_ms, attempts)
    return last
