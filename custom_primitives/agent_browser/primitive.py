from __future__ import annotations

from pathlib import Path
from typing import Any

from primitives.base import (
    load_context,
    run_with_guard,
    validate_http_url,
    validate_int_range,
)
from primitives.types import ActionStatus, PrimitiveResult

from .common import (
    MAX_ATTR_NAME_LENGTH,
    MAX_DIRECTION_LENGTH,
    MAX_JS_LENGTH,
    MAX_KEY_LENGTH,
    MAX_SNAPSHOT_DEPTH,
    MAX_TIMEOUT_MS,
    SUPPORTED_GET_KINDS,
    clear_browser_state,
    derive_default_browser_session,
    extract_first,
    extract_refs,
    extract_text_payload,
    infer_value_from_parsed,
    load_browser_state,
    run_agent_browser,
    save_browser_state,
    utc_now_iso,
    validate_artifact_path,
    validate_cdp_target,
    validate_selector,
    validate_session,
    validate_short_text,
)

WORKFLOW_PATH = Path(__file__).with_name("workflow.md")

ACTION_ALIASES = {
    "click": "click",
    "double_click": "dblclick",
    "dblclick": "dblclick",
    "hover": "hover",
    "focus": "focus",
    "check": "check",
    "uncheck": "uncheck",
    "scroll_into_view": "scrollintoview",
    "scrollintoview": "scrollintoview",
    "fill": "fill",
    "type": "type",
    "select": "select",
    "press": "press",
    "scroll": "scroll",
}
SCROLL_DIRECTIONS = {"up", "down", "left", "right"}
LOAD_STATES = {"load", "domcontentloaded", "networkidle"}
SUPPORTED_ACTIONS = (
    "get_workflow",
    "connect_cdp",
    "open",
    "snapshot",
    "act",
    "wait",
    "get",
    "screenshot",
    "close",
)
SUPPORTED_ACTION_SET = set(SUPPORTED_ACTIONS)


def _build_interaction_command(
    operation: str,
    target: str | None,
    text: str | None,
    value: str | None,
    key: str | None,
    direction: str | None,
    pixels: int | None,
) -> list[str]:
    normalized = ACTION_ALIASES.get(operation)
    if normalized is None:
        raise ValueError(
            "Parameter operation must be one of click, double_click, hover, focus, check, uncheck, scroll_into_view, fill, type, select, press, or scroll"
        )

    if normalized in {
        "click",
        "dblclick",
        "hover",
        "focus",
        "check",
        "uncheck",
        "scrollintoview",
    }:
        if target is None:
            raise ValueError(f"Parameter target is required for operation {operation}")
        return [normalized, target]

    if normalized in {"fill", "type"}:
        if target is None:
            raise ValueError(f"Parameter target is required for operation {operation}")
        if text is None:
            raise ValueError(f"Parameter text is required for operation {operation}")
        return [normalized, target, text]

    if normalized == "select":
        if target is None:
            raise ValueError("Parameter target is required for operation select")
        if value is None:
            raise ValueError("Parameter value is required for operation select")
        return ["select", target, value]

    if normalized == "press":
        if key is None:
            raise ValueError("Parameter key is required for operation press")
        return ["press", key]

    if direction is None:
        raise ValueError("Parameter direction is required for operation scroll")
    if direction not in SCROLL_DIRECTIONS:
        raise ValueError("Parameter direction must be one of up, down, left, or right")
    command = ["scroll", direction]
    if pixels is not None:
        command.append(str(pixels))
    return command


def _build_wait_command(
    selector: str | None,
    milliseconds: int | None,
    url_pattern: str | None,
    load_state: str | None,
    text: str | None,
    js_condition: str | None,
) -> list[str]:
    provided = [
        selector is not None,
        milliseconds is not None,
        url_pattern is not None,
        load_state is not None,
        text is not None,
        js_condition is not None,
    ]
    if sum(provided) != 1:
        raise ValueError(
            "Provide exactly one of selector, milliseconds, url_pattern, load_state, text, or js_condition"
        )
    if selector is not None:
        return ["wait", selector]
    if milliseconds is not None:
        return ["wait", str(milliseconds)]
    if url_pattern is not None:
        return ["wait", "--url", url_pattern]
    if load_state is not None:
        if load_state not in LOAD_STATES:
            raise ValueError(
                "Parameter load_state must be one of load, domcontentloaded, or networkidle"
            )
        return ["wait", "--load", load_state]
    if text is not None:
        return ["wait", "--text", text]
    return ["wait", "--fn", js_condition or ""]


def _build_get_command(kind: str, target: str | None, name: str | None) -> list[str]:
    if kind not in SUPPORTED_GET_KINDS:
        raise ValueError(
            "Parameter kind must be one of text, html, value, attr, title, or url"
        )
    if kind in {"title", "url"}:
        return ["get", kind]
    if target is None:
        raise ValueError(f"Parameter target is required for kind {kind}")
    if kind == "attr":
        if name is None:
            raise ValueError("Parameter name is required for kind attr")
        return ["get", "attr", target, name]
    return ["get", kind, target]


def _invalid_result(
    message: str,
    *,
    error_code: str = "invalid_argument",
    data: dict[str, Any] | None = None,
) -> dict[str, Any]:
    return PrimitiveResult(
        status=ActionStatus.INVALID_ARGUMENT,
        data=data,
        error_code=error_code,
        error_message=message,
        retryable=False,
    ).to_public_dict()


def _stringify_call(action: str, **kwargs: Any) -> str:
    parts = [f'action="{action}"']
    for key, value in kwargs.items():
        if value is None:
            continue
        if isinstance(value, bool):
            rendered = "True" if value else "False"
        elif isinstance(value, (int, float)):
            rendered = str(value)
        else:
            rendered = f'"{value}"'
        parts.append(f"{key}={rendered}")
    return f"agent_browser({', '.join(parts)})"


def _missing_browser_session_result(action: str) -> dict[str, Any]:
    return _invalid_result(
        "No browser session is available for this conversation; connect first or pass session explicitly",
        error_code="missing_browser_session",
        data={
            "action": action,
            "required_parameters": ["session"],
            "example_call": _stringify_call(
                "connect_cdp",
                target="http://127.0.0.1:9222",
                session="browser-main",
            ),
        },
    )


def _missing_required_parameter_result(
    parameter: str,
    *,
    action: str,
    message: str,
    example_call: str,
    extra_data: dict[str, Any] | None = None,
) -> dict[str, Any]:
    data = {
        "action": action,
        "required_parameters": [parameter],
        "example_call": example_call,
    }
    if extra_data:
        data.update(extra_data)
    return _invalid_result(
        message,
        error_code="missing_required_parameter",
        data=data,
    )


def _invalid_parameter_value_result(
    parameter: str,
    *,
    action: str,
    message: str,
    example_call: str,
    extra_data: dict[str, Any] | None = None,
) -> dict[str, Any]:
    data = {
        "action": action,
        "parameter": parameter,
        "example_call": example_call,
    }
    if extra_data:
        data.update(extra_data)
    return _invalid_result(
        message,
        error_code="invalid_parameter_value",
        data=data,
    )


def _stored_browser_session(browser_state: dict[str, Any]) -> str | None:
    raw = browser_state.get("default_browser_session")
    if not isinstance(raw, str):
        return None
    try:
        return validate_session(raw)
    except ValueError:
        return None


def _resolve_browser_session(
    browser_state: dict[str, Any],
    requested_session: str | None,
    *,
    create_default: bool,
) -> str | None:
    if requested_session is not None:
        return requested_session
    stored = _stored_browser_session(browser_state)
    if stored is not None:
        return stored
    if not create_default:
        return None
    return derive_default_browser_session()


def _remember_connected_browser(
    browser_state: dict[str, Any],
    session: str,
    target: str,
) -> None:
    browser_state["default_browser_session"] = session
    browser_state["last_cdp_target"] = target
    browser_state["last_connected_at"] = utc_now_iso()
    save_browser_state(browser_state)


def _clear_connected_browser(browser_state: dict[str, Any]) -> None:
    clear_browser_state()


def _workflow_result() -> dict[str, Any]:
    workflow_md = WORKFLOW_PATH.read_text(encoding="utf-8")
    return PrimitiveResult(
        status=ActionStatus.OK,
        data={
            "name": "agent_browser",
            "format": "markdown",
            "workflow": workflow_md,
        },
        retryable=False,
    ).to_public_dict()


async def agent_browser(
    action: str,
    session: str | None = None,
    headed: bool = False,
    target: str | None = None,
    url: str | None = None,
    interactive_only: bool = True,
    include_cursor: bool = False,
    compact: bool = True,
    selector: str | None = None,
    depth: int | None = None,
    operation: str | None = None,
    text: str | None = None,
    value: str | None = None,
    key: str | None = None,
    direction: str | None = None,
    pixels: int | None = None,
    milliseconds: int | None = None,
    url_pattern: str | None = None,
    load_state: str | None = None,
    js_condition: str | None = None,
    state: str | None = None,
    kind: str | None = None,
    name: str | None = None,
    path: str | None = None,
    full_page: bool = False,
    annotate: bool = False,
) -> dict[str, Any]:
    """
    [What it does]
    Provides the bundled browser automation surface backed by the external agent-browser CLI. Public PTC usage should prefer the namespace methods exposed through help("agent_browser").

    [Args]
    action: One of get_workflow, connect_cdp, open, snapshot, act, wait, get, screenshot, or close.
    session: Optional agent-browser session key.
    headed: Whether to keep the browser headed for actions that open or attach to a page.
    target: CDP endpoint, ref, selector, or wait target depending on action.
    url: Target page URL for action open.
    interactive_only: Whether snapshot should keep only interactive elements.
    include_cursor: Whether snapshot should include cursor-interactive elements.
    compact: Whether snapshot should remove empty structural nodes.
    selector: Optional CSS selector for snapshot or wait.
    depth: Optional snapshot maximum tree depth.
    operation: Interaction kind for action act.
    text: Text payload for action act fill/type or action wait text.
    value: Select value for action act.
    key: Key name for action act press.
    direction: Scroll direction for action act scroll.
    pixels: Scroll amount for action act scroll.
    milliseconds: Fixed wait duration for action wait.
    url_pattern: URL glob pattern for action wait.
    load_state: Load state for action wait.
    js_condition: JavaScript condition for action wait.
    state: Optional selector state for action wait.
    kind: Value kind for action get.
    name: Attribute name for action get kind attr.
    path: Artifact-relative output path for action screenshot.
    full_page: Whether screenshot captures the full page.
    annotate: Whether screenshot overlays interactive labels.

    [Returns]
    PrimitiveResult public dict: status/data/error_code/error_message/retryable

    [Output Contract]
    res = await agent_browser(action="get_workflow")
    assert res["status"] == "ok"
    res["data"]["workflow"]

    [When NOT to use]
    Do not use this when you only need one-shot page fetching without interaction. Prefer the dedicated fetch primitive family for that path.

    [Common Mistakes]
    Use help("agent_browser") to inspect the public namespace methods first. Call action get_workflow when the tool is unfamiliar or may need setup guidance. Do not assume the browser should be closed when the task finishes unless the user explicitly asks for cleanup.
    """
    ctx = load_context()
    try:
        parsed_action = validate_short_text(action, "action", max_length=32).lower()
        if parsed_action not in SUPPORTED_ACTION_SET:
            return _invalid_parameter_value_result(
                "action",
                action=parsed_action,
                message=(
                    "Parameter action must be one of get_workflow, connect_cdp, open, "
                    "snapshot, act, wait, get, screenshot, or close"
                ),
                example_call=_stringify_call("get_workflow"),
                extra_data={"allowed_actions": list(SUPPORTED_ACTIONS)},
            )
        parsed_session = validate_session(session)
        if not isinstance(headed, bool):
            raise ValueError("Parameter headed must be a boolean")
        browser_state = load_browser_state()
        if parsed_action == "get_workflow":
            return _workflow_result()

        if parsed_action == "connect_cdp":
            parsed_target = validate_cdp_target(target)
            resolved_session = (
                parsed_session
                or _stored_browser_session(browser_state)
                or derive_default_browser_session()
            )
            result = await run_with_guard(
                primitive_name="agent_browser",
                ctx=ctx,
                operation=lambda: run_agent_browser(
                    ["--cdp", parsed_target, "get", "cdp-url"],
                    session=resolved_session,
                    headed=headed,
                ),
                max_retries=0,
            )
            if result.status == ActionStatus.OK and isinstance(result.data, dict):
                if resolved_session is not None:
                    _remember_connected_browser(browser_state, resolved_session, parsed_target)
                result.data["action"] = parsed_action
                result.data["target"] = parsed_target
                result.data["session"] = resolved_session
                result.data["cdp_url"] = extract_first(
                    result.data.get("parsed"),
                    ("cdpUrl",),
                    ("data", "cdpUrl"),
                ) or infer_value_from_parsed(result.data.get("parsed"))
                result.data["url"] = extract_first(
                    result.data.get("parsed"),
                    ("url",),
                    ("browserUrl",),
                    ("data", "url"),
                )
            return result.to_public_dict()

        if parsed_action == "open":
            parsed_url = validate_http_url(url or "", "url")
            resolved_session = _resolve_browser_session(
                browser_state,
                parsed_session,
                create_default=False,
            )
            if resolved_session is None:
                return _missing_browser_session_result(parsed_action)
            result = await run_with_guard(
                primitive_name="agent_browser",
                ctx=ctx,
                operation=lambda: run_agent_browser(
                    ["open", parsed_url],
                    session=resolved_session,
                    headed=headed,
                ),
                max_retries=0,
            )
            if result.status == ActionStatus.OK and isinstance(result.data, dict):
                result.data["action"] = parsed_action
                result.data["opened_url"] = parsed_url
                result.data["session"] = resolved_session
                result.data["url"] = extract_first(
                    result.data.get("parsed"),
                    ("url",),
                    ("data", "url"),
                ) or parsed_url
                result.data["title"] = extract_first(
                    result.data.get("parsed"),
                    ("title",),
                    ("data", "title"),
                )
            return result.to_public_dict()

        if parsed_action == "snapshot":
            if not isinstance(interactive_only, bool):
                raise ValueError("Parameter interactive_only must be a boolean")
            if not isinstance(include_cursor, bool):
                raise ValueError("Parameter include_cursor must be a boolean")
            if not isinstance(compact, bool):
                raise ValueError("Parameter compact must be a boolean")
            parsed_selector = (
                validate_selector(selector, "selector") if selector is not None else None
            )
            parsed_depth = (
                validate_int_range(depth, "depth", 1, MAX_SNAPSHOT_DEPTH)
                if depth is not None
                else None
            )
            resolved_session = _resolve_browser_session(
                browser_state,
                parsed_session,
                create_default=False,
            )
            if resolved_session is None:
                return _missing_browser_session_result(parsed_action)
            command = ["snapshot"]
            if interactive_only:
                command.append("-i")
            if include_cursor:
                command.append("-C")
            if compact:
                command.append("-c")
            if parsed_depth is not None:
                command.extend(["-d", str(parsed_depth)])
            if parsed_selector:
                command.extend(["-s", parsed_selector])
            result = await run_with_guard(
                primitive_name="agent_browser",
                ctx=ctx,
                operation=lambda: run_agent_browser(command, session=resolved_session),
                max_retries=0,
            )
            if result.status == ActionStatus.OK and isinstance(result.data, dict):
                snapshot = extract_text_payload(result.data)
                result.data["action"] = parsed_action
                result.data["session"] = resolved_session
                result.data["snapshot"] = snapshot
                result.data["refs"] = extract_refs(snapshot)
            return result.to_public_dict()

        if parsed_action == "act":
            parsed_operation = validate_short_text(
                operation, "operation", max_length=64
            ).lower()
            parsed_target = validate_selector(target, "target") if target is not None else None
            parsed_text = validate_short_text(text, "text") if text is not None else None
            parsed_value = validate_short_text(value, "value") if value is not None else None
            parsed_key = (
                validate_short_text(key, "key", max_length=MAX_KEY_LENGTH)
                if key is not None
                else None
            )
            parsed_direction = (
                validate_short_text(direction, "direction", max_length=MAX_DIRECTION_LENGTH)
                .lower()
                if direction is not None
                else None
            )
            parsed_pixels = (
                validate_int_range(pixels, "pixels", 1, 100000)
                if pixels is not None
                else None
            )
            resolved_session = _resolve_browser_session(
                browser_state,
                parsed_session,
                create_default=False,
            )
            if resolved_session is None:
                return _missing_browser_session_result(parsed_action)
            command = _build_interaction_command(
                parsed_operation,
                parsed_target,
                parsed_text,
                parsed_value,
                parsed_key,
                parsed_direction,
                parsed_pixels,
            )
            result = await run_with_guard(
                primitive_name="agent_browser",
                ctx=ctx,
                operation=lambda: run_agent_browser(command, session=resolved_session),
                max_retries=0,
            )
            if result.status == ActionStatus.OK and isinstance(result.data, dict):
                result.data["action"] = parsed_action
                result.data["operation"] = parsed_operation
                result.data["session"] = resolved_session
                result.data["target"] = parsed_target
            return result.to_public_dict()

        if parsed_action == "wait":
            if selector is not None and target is not None:
                raise ValueError("Use only one of selector or target for action wait")
            parsed_selector = (
                validate_selector(selector, "selector") if selector is not None else None
            )
            parsed_target = validate_selector(target, "target") if target is not None else None
            wait_target = parsed_selector or parsed_target
            parsed_milliseconds = (
                validate_int_range(milliseconds, "milliseconds", 0, MAX_TIMEOUT_MS)
                if milliseconds is not None
                else None
            )
            parsed_url_pattern = (
                validate_short_text(url_pattern, "url_pattern")
                if url_pattern is not None
                else None
            )
            parsed_load_state = (
                validate_short_text(load_state, "load_state", max_length=32).lower()
                if load_state is not None
                else None
            )
            parsed_text = validate_short_text(text, "text") if text is not None else None
            parsed_js_condition = (
                validate_short_text(js_condition, "js_condition", max_length=MAX_JS_LENGTH)
                if js_condition is not None
                else None
            )
            parsed_state = (
                validate_short_text(state, "state", max_length=32).lower()
                if state is not None
                else None
            )
            if parsed_state is not None and wait_target is None:
                raise ValueError("Parameter state requires selector or target for action wait")
            resolved_session = _resolve_browser_session(
                browser_state,
                parsed_session,
                create_default=False,
            )
            if resolved_session is None:
                return _missing_browser_session_result(parsed_action)
            command = _build_wait_command(
                wait_target,
                parsed_milliseconds,
                parsed_url_pattern,
                parsed_load_state,
                parsed_text,
                parsed_js_condition,
            )
            if wait_target is not None and parsed_state is not None:
                command.extend(["--state", parsed_state])
            result = await run_with_guard(
                primitive_name="agent_browser",
                ctx=ctx,
                operation=lambda: run_agent_browser(command, session=resolved_session),
                max_retries=0,
            )
            if result.status == ActionStatus.OK and isinstance(result.data, dict):
                result.data["action"] = parsed_action
                result.data["session"] = resolved_session
            return result.to_public_dict()

        if parsed_action == "get":
            if kind is None or not str(kind).strip():
                return _missing_required_parameter_result(
                    "kind",
                    action=parsed_action,
                    message="Parameter kind is required for action get",
                    example_call=_stringify_call("get", kind="title"),
                    extra_data={"allowed_kind_values": list(SUPPORTED_GET_KINDS)},
                )
            parsed_kind = validate_short_text(kind, "kind", max_length=32).lower()
            if parsed_kind not in SUPPORTED_GET_KINDS:
                return _invalid_parameter_value_result(
                    "kind",
                    action=parsed_action,
                    message="Parameter kind must be one of text, html, value, attr, title, or url",
                    example_call=_stringify_call("get", kind="title"),
                    extra_data={"allowed_kind_values": list(SUPPORTED_GET_KINDS)},
                )
            parsed_target = validate_selector(target, "target") if target is not None else None
            parsed_name = (
                validate_short_text(name, "name", max_length=MAX_ATTR_NAME_LENGTH)
                if name is not None
                else None
            )
            if parsed_kind in {"text", "html", "value"} and parsed_target is None:
                return _missing_required_parameter_result(
                    "target",
                    action=parsed_action,
                    message=f"Parameter target is required for kind {parsed_kind}",
                    example_call=_stringify_call("get", kind=parsed_kind, target="@e1"),
                    extra_data={"allowed_kind_values": list(SUPPORTED_GET_KINDS)},
                )
            if parsed_kind == "attr":
                if parsed_target is None:
                    return _missing_required_parameter_result(
                        "target",
                        action=parsed_action,
                        message="Parameter target is required for kind attr",
                        example_call=_stringify_call(
                            "get",
                            kind="attr",
                            target="@e1",
                            name="href",
                        ),
                        extra_data={"allowed_kind_values": list(SUPPORTED_GET_KINDS)},
                    )
                if parsed_name is None:
                    return _missing_required_parameter_result(
                        "name",
                        action=parsed_action,
                        message="Parameter name is required for kind attr",
                        example_call=_stringify_call(
                            "get",
                            kind="attr",
                            target="@e1",
                            name="href",
                        ),
                        extra_data={"allowed_kind_values": list(SUPPORTED_GET_KINDS)},
                    )
            resolved_session = _resolve_browser_session(
                browser_state,
                parsed_session,
                create_default=False,
            )
            if resolved_session is None:
                return _missing_browser_session_result(parsed_action)
            command = _build_get_command(parsed_kind, parsed_target, parsed_name)
            result = await run_with_guard(
                primitive_name="agent_browser",
                ctx=ctx,
                operation=lambda: run_agent_browser(command, session=resolved_session),
                max_retries=0,
            )
            if result.status == ActionStatus.OK and isinstance(result.data, dict):
                result.data["action"] = parsed_action
                result.data["kind"] = parsed_kind
                result.data["session"] = resolved_session
                result.data["target"] = parsed_target
                result.data["value"] = infer_value_from_parsed(result.data.get("parsed"))
            return result.to_public_dict()

        if parsed_action == "screenshot":
            if not isinstance(full_page, bool):
                raise ValueError("Parameter full_page must be a boolean")
            if not isinstance(annotate, bool):
                raise ValueError("Parameter annotate must be a boolean")
            artifact_path = validate_artifact_path(path, "agent-browser-screenshot.png")
            resolved_session = _resolve_browser_session(
                browser_state,
                parsed_session,
                create_default=False,
            )
            if resolved_session is None:
                return _missing_browser_session_result(parsed_action)
            command = ["screenshot"]
            if full_page:
                command.append("--full")
            if annotate:
                command.append("--annotate")
            command.append(artifact_path)
            result = await run_with_guard(
                primitive_name="agent_browser",
                ctx=ctx,
                operation=lambda: run_agent_browser(command, session=resolved_session),
                max_retries=0,
            )
            if result.status == ActionStatus.OK and isinstance(result.data, dict):
                result.data["action"] = parsed_action
                result.data["session"] = resolved_session
                result.data["artifact_path"] = artifact_path
            return result.to_public_dict()

        resolved_session = _resolve_browser_session(
            browser_state,
            parsed_session,
            create_default=False,
        )
        if resolved_session is None:
            return _missing_browser_session_result(parsed_action)
        result = await run_with_guard(
            primitive_name="agent_browser",
            ctx=ctx,
            operation=lambda: run_agent_browser(["close"], session=resolved_session),
            max_retries=0,
        )
        if result.status == ActionStatus.OK and isinstance(result.data, dict):
            _clear_connected_browser(browser_state)
            result.data["action"] = parsed_action
            result.data["session"] = resolved_session
            result.data["closed"] = True
        return result.to_public_dict()
    except ValueError as exc:
        return _invalid_result(str(exc))
