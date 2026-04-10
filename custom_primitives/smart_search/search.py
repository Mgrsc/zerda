from __future__ import annotations

import json
import os
import socket
from typing import Any
from urllib.error import HTTPError, URLError
from urllib.request import Request, urlopen

from primitives.base import (
    HARD_NETWORK_TIMEOUT_SECS,
    PrimitiveContext,
    dependency_missing_result,
    invalid_argument_result,
    load_context,
    parse_json_bytes,
    run_with_guard,
    validate_http_url,
    validate_int_range,
)
from primitives.types import ActionStatus, PrimitiveResult

MAX_QUERY_LENGTH = 4000
MAX_MODEL_LENGTH = 200
MAX_RESPONSE_TOKENS = 32768
DEFAULT_MAX_TOKENS = 4096


def _as_dict(raw: Any) -> dict[str, Any]:
    if isinstance(raw, dict):
        return raw
    return {}


def _validate_query(raw: Any) -> str:
    value = str(raw or "").strip()
    if not value:
        raise ValueError("Parameter query must not be empty")
    if len(value) > MAX_QUERY_LENGTH:
        raise ValueError(f"Parameter query exceeds max length {MAX_QUERY_LENGTH}")
    return value


def _validate_model(raw: Any) -> str:
    value = str(raw or "").strip()
    if not value:
        raise ValueError("Parameter model must not be empty")
    if len(value) > MAX_MODEL_LENGTH:
        raise ValueError(f"Parameter model exceeds max length {MAX_MODEL_LENGTH}")
    return value


def _resolve_setting(parameter_value: str | None, env_name: str) -> str:
    value = str(parameter_value or "").strip()
    if value:
        return value
    return os.environ.get(env_name, "").strip()


def _resolve_config(
    *,
    url: str | None,
    api_key: str | None,
    model: str | None,
) -> tuple[str, str, str] | PrimitiveResult:
    resolved_url = _resolve_setting(url, "SMART_SEARCH_URL")
    resolved_api_key = _resolve_setting(api_key, "SMART_SEARCH_API_KEY")
    resolved_model = _resolve_setting(model, "SMART_SEARCH_MODEL")

    missing = [
        name
        for name, value in (
            ("SMART_SEARCH_URL", resolved_url),
            ("SMART_SEARCH_API_KEY", resolved_api_key),
            ("SMART_SEARCH_MODEL", resolved_model),
        )
        if not value
    ]
    if missing:
        return dependency_missing_result(
            f"Missing {', '.join(missing)}; configure the OpenAI-compatible chat search primitive before use"
        )

    try:
        parsed_url = validate_http_url(resolved_url, "url")
        parsed_model = _validate_model(resolved_model)
    except ValueError as exc:
        return invalid_argument_result(str(exc))
    return parsed_url, resolved_api_key, parsed_model


def _extract_text_content(raw: Any) -> str:
    if isinstance(raw, str):
        return raw.strip()
    if not isinstance(raw, list):
        return ""
    parts: list[str] = []
    for item in raw:
        if isinstance(item, str):
            value = item.strip()
            if value:
                parts.append(value)
            continue
        if not isinstance(item, dict):
            continue
        text = item.get("text")
        if isinstance(text, str) and text.strip():
            parts.append(text.strip())
    return "\n\n".join(parts)


def _post_chat_completion(
    *,
    endpoint_url: str,
    api_key: str,
    payload: dict[str, Any],
    timeout_secs: float,
) -> PrimitiveResult:
    body = json.dumps(payload, ensure_ascii=False).encode("utf-8")
    request = Request(url=endpoint_url, data=body, method="POST")
    request.add_header("Authorization", f"Bearer {api_key}")
    request.add_header("Content-Type", "application/json")
    request.add_header("Accept", "application/json")
    request.add_header("User-Agent", "zerda-openai-chat-search")
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
                error_message=f"Chat completions endpoint returned 429: {parsed}",
                retryable=True,
                telemetry={"http_status": status_code},
            )
        return PrimitiveResult(
            status=ActionStatus.UPSTREAM_ERROR,
            error_code="upstream_http_error",
            error_message=f"Chat completions endpoint returned {status_code}: {parsed}",
            retryable=status_code >= 500,
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


def _normalize_chat_data(raw: Any, *, fallback_model: str) -> dict[str, Any]:
    envelope = _as_dict(raw)
    payload = _as_dict(envelope.get("result"))
    choices = payload.get("choices")
    first_choice = choices[0] if isinstance(choices, list) and choices else {}
    first_choice_dict = _as_dict(first_choice)
    message = _as_dict(first_choice_dict.get("message"))
    answer = _extract_text_content(message.get("content"))
    return {
        "answer": answer,
        "message": message,
        "finish_reason": first_choice_dict.get("finish_reason"),
        "model": payload.get("model") or fallback_model,
        "usage": _as_dict(payload.get("usage")),
        "http_status": envelope.get("http_status"),
        "id": payload.get("id"),
        "object": payload.get("object"),
        "raw_response": payload,
    }


def _search_operation(
    ctx: PrimitiveContext,
    *,
    endpoint_url: str,
    api_key: str,
    model: str,
    query: str,
    max_tokens: int,
) -> PrimitiveResult:
    del ctx
    payload = {
        "model": model,
        "stream": False,
        "max_tokens": max_tokens,
        "messages": [
            {
                "role": "user",
                "content": query,
            }
        ],
    }
    return _post_chat_completion(
        endpoint_url=endpoint_url,
        api_key=api_key,
        payload=payload,
        timeout_secs=HARD_NETWORK_TIMEOUT_SECS,
    )


async def smart_search(
    query: str,
    url: str | None = None,
    api_key: str | None = None,
    model: str | None = None,
    max_tokens: int = DEFAULT_MAX_TOKENS,
) -> dict[str, Any]:
    """
    [What it does]
    Calls a non-streaming OpenAI-compatible chat completions endpoint and returns the first assistant answer for information retrieval.

    [Args]
    query: User question sent as the single user message.
    url: Full chat completions endpoint URL. Defaults to SMART_SEARCH_URL.
    api_key: Bearer token for the endpoint. Defaults to SMART_SEARCH_API_KEY.
    model: Model name. Defaults to SMART_SEARCH_MODEL.
    max_tokens: Maximum completion tokens (1~32768, default 4096).

    [Output Contract]
    res = await smart_search("compare hermes agent and openclaw")
    assert res["status"] == "ok"
    res["data"]["answer"]
    res["data"]["usage"]

    [When NOT to use]
    Do not use this when you already know the target page URL and only need deterministic page fetching or browser interaction.
    """
    ctx = load_context()
    try:
        parsed_query = _validate_query(query)
        parsed_max_tokens = validate_int_range(
            max_tokens,
            "max_tokens",
            1,
            MAX_RESPONSE_TOKENS,
        )
    except ValueError as exc:
        return invalid_argument_result(str(exc)).to_public_dict()

    resolved = _resolve_config(url=url, api_key=api_key, model=model)
    if isinstance(resolved, PrimitiveResult):
        return resolved.to_public_dict()
    endpoint_url, resolved_api_key, resolved_model = resolved

    result = await run_with_guard(
        primitive_name="smart_search",
        ctx=ctx,
        operation=lambda: _search_operation(
            ctx,
            endpoint_url=endpoint_url,
            api_key=resolved_api_key,
            model=resolved_model,
            query=parsed_query,
            max_tokens=parsed_max_tokens,
        ),
    )
    if result.status == ActionStatus.OK:
        result.data = _normalize_chat_data(result.data, fallback_model=resolved_model)
    return result.to_public_dict()
