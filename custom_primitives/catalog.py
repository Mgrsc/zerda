from __future__ import annotations

from typing import Any, Awaitable, Callable

from .agent_browser.api import (
    agent_browser_check,
    agent_browser_click,
    agent_browser_close,
    agent_browser_connect_cdp,
    agent_browser_double_click,
    agent_browser_fill,
    agent_browser_focus,
    agent_browser_get_attr,
    agent_browser_get_html,
    agent_browser_get_text,
    agent_browser_get_title,
    agent_browser_get_url,
    agent_browser_get_value,
    agent_browser_get_workflow,
    agent_browser_hover,
    agent_browser_open,
    agent_browser_press,
    agent_browser_scroll,
    agent_browser_scroll_into_view,
    agent_browser_select,
    agent_browser_sleep,
    agent_browser_snapshot,
    agent_browser_screenshot,
    agent_browser_type,
    agent_browser_uncheck,
    agent_browser_wait_for_js,
    agent_browser_wait_for_load,
    agent_browser_wait_for_selector,
    agent_browser_wait_for_text,
    agent_browser_wait_for_url,
)
from .extract_main_text_from_html import extract_main_text_from_html
from .firecrawl import firecrawl_search_web
from .scrapling import scrapling_fetch_page

PrimitiveCallable = Callable[..., Awaitable[dict[str, Any]]]


def get_primitive_registry(
    disabled_primitives: set[str] | None = None,
) -> dict[str, PrimitiveCallable]:
    disabled = disabled_primitives or set()
    registry: dict[str, PrimitiveCallable] = {
        "agent_browser_check": agent_browser_check,
        "agent_browser_click": agent_browser_click,
        "agent_browser_close": agent_browser_close,
        "agent_browser_connect_cdp": agent_browser_connect_cdp,
        "agent_browser_double_click": agent_browser_double_click,
        "agent_browser_fill": agent_browser_fill,
        "agent_browser_focus": agent_browser_focus,
        "agent_browser_get_attr": agent_browser_get_attr,
        "agent_browser_get_html": agent_browser_get_html,
        "agent_browser_get_text": agent_browser_get_text,
        "agent_browser_get_title": agent_browser_get_title,
        "agent_browser_get_url": agent_browser_get_url,
        "agent_browser_get_value": agent_browser_get_value,
        "agent_browser_get_workflow": agent_browser_get_workflow,
        "agent_browser_hover": agent_browser_hover,
        "agent_browser_open": agent_browser_open,
        "agent_browser_press": agent_browser_press,
        "agent_browser_scroll": agent_browser_scroll,
        "agent_browser_scroll_into_view": agent_browser_scroll_into_view,
        "agent_browser_select": agent_browser_select,
        "agent_browser_sleep": agent_browser_sleep,
        "agent_browser_snapshot": agent_browser_snapshot,
        "agent_browser_screenshot": agent_browser_screenshot,
        "agent_browser_type": agent_browser_type,
        "agent_browser_uncheck": agent_browser_uncheck,
        "agent_browser_wait_for_js": agent_browser_wait_for_js,
        "agent_browser_wait_for_load": agent_browser_wait_for_load,
        "agent_browser_wait_for_selector": agent_browser_wait_for_selector,
        "agent_browser_wait_for_text": agent_browser_wait_for_text,
        "agent_browser_wait_for_url": agent_browser_wait_for_url,
        "extract_main_text_from_html": extract_main_text_from_html,
        "firecrawl_search_web": firecrawl_search_web,
        "scrapling_fetch_page": scrapling_fetch_page,
    }
    return {name: fn for name, fn in registry.items() if name not in disabled}
