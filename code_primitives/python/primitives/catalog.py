from __future__ import annotations

from typing import Any, Awaitable, Callable

from .extract_main_text_from_html import extract_main_text_from_html
from .firecrawl_scrape_page import firecrawl_scrape_page
from .firecrawl_search_web import firecrawl_search_web

PrimitiveCallable = Callable[..., Awaitable[dict[str, Any]]]


def get_primitive_registry(enable_firecrawl: bool) -> dict[str, PrimitiveCallable]:
    registry: dict[str, PrimitiveCallable] = {
        "extract_main_text_from_html": extract_main_text_from_html,
    }
    if enable_firecrawl:
        registry["firecrawl_scrape_page"] = firecrawl_scrape_page
        registry["firecrawl_search_web"] = firecrawl_search_web
    return registry
