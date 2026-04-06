from __future__ import annotations

from typing import Any

from .primitive import agent_browser as legacy_agent_browser


async def agent_browser_get_workflow() -> dict[str, Any]:
    """
    [What it does]
    Return the end-to-end workflow for installing and using the browser tool safely.

    [Args]
    None

    [Output Contract]
    Returns the standard primitive result. On success, data.workflow contains Markdown guidance for setup, installation, and operational order.

    [When NOT to use]
    Do not use this when you already know the browser workflow and only need one direct action.

    [Common Mistakes]
    Call this when the tool is unfamiliar, when setup might be missing, or when you need installation guidance.
    """
    return await legacy_agent_browser(action="get_workflow")


async def agent_browser_connect_cdp(
    target: str,
    session: str | None = None,
    headed: bool = False,
) -> dict[str, Any]:
    """
    [What it does]
    Attach to an existing browser that the user exposed through a CDP port or endpoint.

    [Args]
    target: CDP port, HTTP endpoint, or WebSocket endpoint.
    session: Optional browser session key to reuse later.
    headed: Keep the browser headed while attaching.

    [Output Contract]
    Returns the standard primitive result. On success, data may include the resolved session and CDP URL.

    [When NOT to use]
    Do not use this for static page fetching or one-off HTML retrieval.

    [Common Mistakes]
    If agent-browser is not installed yet, inspect agent_browser.get_workflow first and follow the setup steps.
    """
    return await legacy_agent_browser(
        action="connect_cdp",
        target=target,
        session=session,
        headed=headed,
    )


async def agent_browser_open(
    url: str,
    session: str | None = None,
    headed: bool = False,
) -> dict[str, Any]:
    """
    [What it does]
    Navigate the attached browser to a new URL.

    [Args]
    url: HTTP or HTTPS URL to open.
    session: Optional browser session key.
    headed: Keep the browser headed while opening.

    [Output Contract]
    Returns the standard primitive result. On success, data may include the opened URL and page title.

    [When NOT to use]
    Do not use this before connecting to a browser unless you intentionally pass a reusable session.

    [Common Mistakes]
    Reuse the same session after connect_cdp so later actions stay attached to the intended browser.
    """
    return await legacy_agent_browser(
        action="open",
        url=url,
        session=session,
        headed=headed,
    )


async def agent_browser_snapshot(
    session: str | None = None,
    interactive_only: bool = True,
    include_cursor: bool = False,
    compact: bool = True,
    selector: str | None = None,
    depth: int | None = None,
) -> dict[str, Any]:
    """
    [What it does]
    Inspect the current page and return a compact snapshot with interactive refs.

    [Args]
    session: Optional browser session key.
    interactive_only: Keep only interactive elements in the snapshot.
    include_cursor: Include cursor-interactive nodes.
    compact: Remove empty structural nodes.
    selector: Optional CSS selector to scope the snapshot.
    depth: Optional maximum snapshot depth.

    [Output Contract]
    Returns the standard primitive result. On success, data.snapshot contains the tree and data.refs contains extracted refs such as @e1.

    [When NOT to use]
    Do not use this when you only need one stable page value such as title or URL.

    [Common Mistakes]
    Take a fresh snapshot after navigation or meaningful DOM changes before reusing old refs.
    """
    return await legacy_agent_browser(
        action="snapshot",
        session=session,
        interactive_only=interactive_only,
        include_cursor=include_cursor,
        compact=compact,
        selector=selector,
        depth=depth,
    )


async def agent_browser_click(
    target: str,
    session: str | None = None,
) -> dict[str, Any]:
    """
    [What it does]
    Click an element in the attached browser.

    [Args]
    target: Snapshot ref or selector to click.
    session: Optional browser session key.

    [Output Contract]
    Returns the standard primitive result for one click action.

    [When NOT to use]
    Do not use this when the target element reference is stale after navigation.

    [Common Mistakes]
    Prefer refs from a fresh snapshot instead of guessing selectors.
    """
    return await legacy_agent_browser(
        action="act",
        operation="click",
        target=target,
        session=session,
    )


async def agent_browser_double_click(
    target: str,
    session: str | None = None,
) -> dict[str, Any]:
    """
    [What it does]
    Double-click an element in the attached browser.

    [Args]
    target: Snapshot ref or selector to double-click.
    session: Optional browser session key.

    [Output Contract]
    Returns the standard primitive result for one double-click action.

    [When NOT to use]
    Do not use this when a normal click is enough.

    [Common Mistakes]
    Use this only when the page actually requires a double-click interaction.
    """
    return await legacy_agent_browser(
        action="act",
        operation="double_click",
        target=target,
        session=session,
    )


async def agent_browser_hover(
    target: str,
    session: str | None = None,
) -> dict[str, Any]:
    """
    [What it does]
    Hover an element in the attached browser.

    [Args]
    target: Snapshot ref or selector to hover.
    session: Optional browser session key.

    [Output Contract]
    Returns the standard primitive result for one hover action.

    [When NOT to use]
    Do not use this if the page does not react to hover state.

    [Common Mistakes]
    Snapshot again after hover if the page reveals new interactive elements.
    """
    return await legacy_agent_browser(
        action="act",
        operation="hover",
        target=target,
        session=session,
    )


async def agent_browser_focus(
    target: str,
    session: str | None = None,
) -> dict[str, Any]:
    """
    [What it does]
    Focus an element in the attached browser.

    [Args]
    target: Snapshot ref or selector to focus.
    session: Optional browser session key.

    [Output Contract]
    Returns the standard primitive result for one focus action.

    [When NOT to use]
    Do not use this when fill or click already handles the needed interaction.

    [Common Mistakes]
    Focus is useful for keyboard-driven flows before press or type.
    """
    return await legacy_agent_browser(
        action="act",
        operation="focus",
        target=target,
        session=session,
    )


async def agent_browser_check(
    target: str,
    session: str | None = None,
) -> dict[str, Any]:
    """
    [What it does]
    Check a checkbox or similar toggle element.

    [Args]
    target: Snapshot ref or selector to check.
    session: Optional browser session key.

    [Output Contract]
    Returns the standard primitive result for one check action.

    [When NOT to use]
    Do not use this for ordinary buttons.

    [Common Mistakes]
    Use check or uncheck instead of click when the target is a checkbox-like control.
    """
    return await legacy_agent_browser(
        action="act",
        operation="check",
        target=target,
        session=session,
    )


async def agent_browser_uncheck(
    target: str,
    session: str | None = None,
) -> dict[str, Any]:
    """
    [What it does]
    Uncheck a checkbox or similar toggle element.

    [Args]
    target: Snapshot ref or selector to uncheck.
    session: Optional browser session key.

    [Output Contract]
    Returns the standard primitive result for one uncheck action.

    [When NOT to use]
    Do not use this for ordinary buttons.

    [Common Mistakes]
    Use check or uncheck instead of click when the target is a checkbox-like control.
    """
    return await legacy_agent_browser(
        action="act",
        operation="uncheck",
        target=target,
        session=session,
    )


async def agent_browser_scroll_into_view(
    target: str,
    session: str | None = None,
) -> dict[str, Any]:
    """
    [What it does]
    Scroll the page until the target element is in view.

    [Args]
    target: Snapshot ref or selector to scroll into view.
    session: Optional browser session key.

    [Output Contract]
    Returns the standard primitive result for one scroll-into-view action.

    [When NOT to use]
    Do not use this for freeform page scrolling without a target.

    [Common Mistakes]
    Use this before clicking elements that are outside the viewport.
    """
    return await legacy_agent_browser(
        action="act",
        operation="scroll_into_view",
        target=target,
        session=session,
    )


async def agent_browser_fill(
    target: str,
    text: str,
    session: str | None = None,
) -> dict[str, Any]:
    """
    [What it does]
    Fill an input-like element with replacement text.

    [Args]
    target: Snapshot ref or selector to fill.
    text: Text to place into the target.
    session: Optional browser session key.

    [Output Contract]
    Returns the standard primitive result for one fill action.

    [When NOT to use]
    Do not use this when the page requires literal keypress typing behavior.

    [Common Mistakes]
    Use type instead of fill if the page reacts to each keystroke.
    """
    return await legacy_agent_browser(
        action="act",
        operation="fill",
        target=target,
        text=text,
        session=session,
    )


async def agent_browser_type(
    target: str,
    text: str,
    session: str | None = None,
) -> dict[str, Any]:
    """
    [What it does]
    Type text into an input-like element as keyboard input.

    [Args]
    target: Snapshot ref or selector to type into.
    text: Text to type.
    session: Optional browser session key.

    [Output Contract]
    Returns the standard primitive result for one type action.

    [When NOT to use]
    Do not use this when a plain fill is enough and faster.

    [Common Mistakes]
    Use type when the page attaches listeners that react to each keystroke.
    """
    return await legacy_agent_browser(
        action="act",
        operation="type",
        target=target,
        text=text,
        session=session,
    )


async def agent_browser_select(
    target: str,
    value: str,
    session: str | None = None,
) -> dict[str, Any]:
    """
    [What it does]
    Select an option value from a select-like control.

    [Args]
    target: Snapshot ref or selector to select within.
    value: Option value to choose.
    session: Optional browser session key.

    [Output Contract]
    Returns the standard primitive result for one select action.

    [When NOT to use]
    Do not use this for freeform text inputs.

    [Common Mistakes]
    Ensure the provided value matches an actual option value exposed by the page.
    """
    return await legacy_agent_browser(
        action="act",
        operation="select",
        target=target,
        value=value,
        session=session,
    )


async def agent_browser_press(
    key: str,
    session: str | None = None,
) -> dict[str, Any]:
    """
    [What it does]
    Send one keyboard key press to the attached browser.

    [Args]
    key: Key name to press.
    session: Optional browser session key.

    [Output Contract]
    Returns the standard primitive result for one key press.

    [When NOT to use]
    Do not use this when you need to click or type into a specific element first.

    [Common Mistakes]
    Focus the right element before pressing keys if the page routes input by focus state.
    """
    return await legacy_agent_browser(
        action="act",
        operation="press",
        key=key,
        session=session,
    )


async def agent_browser_scroll(
    direction: str,
    session: str | None = None,
    pixels: int | None = None,
) -> dict[str, Any]:
    """
    [What it does]
    Scroll the page in one direction, optionally by a fixed pixel amount.

    [Args]
    direction: Scroll direction such as up, down, left, or right.
    session: Optional browser session key.
    pixels: Optional scroll amount.

    [Output Contract]
    Returns the standard primitive result for one scroll action.

    [When NOT to use]
    Do not use this when a specific element should be scrolled into view directly.

    [Common Mistakes]
    Use scroll_into_view when you already know the target element.
    """
    return await legacy_agent_browser(
        action="act",
        operation="scroll",
        direction=direction,
        pixels=pixels,
        session=session,
    )


async def agent_browser_wait_for_selector(
    target: str,
    session: str | None = None,
    state: str | None = None,
) -> dict[str, Any]:
    """
    [What it does]
    Wait until a selector or snapshot ref reaches the desired state.

    [Args]
    target: Selector or ref to wait on.
    session: Optional browser session key.
    state: Optional selector state such as visible or attached.

    [Output Contract]
    Returns the standard primitive result once the wait condition succeeds or times out upstream.

    [When NOT to use]
    Do not use this for URL, text, load-state, or fixed-duration waits.

    [Common Mistakes]
    Prefer specific waits like selector or load state instead of fixed sleeps when possible.
    """
    return await legacy_agent_browser(
        action="wait",
        target=target,
        state=state,
        session=session,
    )


async def agent_browser_wait_for_load(
    load_state: str,
    session: str | None = None,
) -> dict[str, Any]:
    """
    [What it does]
    Wait until the page reaches a target load state.

    [Args]
    load_state: Load state such as load, domcontentloaded, or networkidle.
    session: Optional browser session key.

    [Output Contract]
    Returns the standard primitive result once the load state is reached or upstream waiting fails.

    [When NOT to use]
    Do not use this when you need a selector-specific or text-specific wait.

    [Common Mistakes]
    Use networkidle only when the page genuinely quiets down after loading.
    """
    return await legacy_agent_browser(
        action="wait",
        load_state=load_state,
        session=session,
    )


async def agent_browser_wait_for_url(
    url_pattern: str,
    session: str | None = None,
) -> dict[str, Any]:
    """
    [What it does]
    Wait until the current URL matches a pattern.

    [Args]
    url_pattern: URL glob pattern to wait for.
    session: Optional browser session key.

    [Output Contract]
    Returns the standard primitive result once the URL matches or upstream waiting fails.

    [When NOT to use]
    Do not use this when the page change is better detected by selector or load-state signals.

    [Common Mistakes]
    Use this after redirects or route transitions that do not expose a stable selector immediately.
    """
    return await legacy_agent_browser(
        action="wait",
        url_pattern=url_pattern,
        session=session,
    )


async def agent_browser_wait_for_text(
    text: str,
    session: str | None = None,
) -> dict[str, Any]:
    """
    [What it does]
    Wait until the page contains target text.

    [Args]
    text: Text to wait for.
    session: Optional browser session key.

    [Output Contract]
    Returns the standard primitive result once the text appears or upstream waiting fails.

    [When NOT to use]
    Do not use this for element-specific waits when you already know a stable selector.

    [Common Mistakes]
    Prefer narrower waits when the target text is common on the page.
    """
    return await legacy_agent_browser(
        action="wait",
        text=text,
        session=session,
    )


async def agent_browser_wait_for_js(
    js_condition: str,
    session: str | None = None,
) -> dict[str, Any]:
    """
    [What it does]
    Wait until a JavaScript condition becomes true.

    [Args]
    js_condition: JavaScript condition expression to evaluate.
    session: Optional browser session key.

    [Output Contract]
    Returns the standard primitive result once the condition becomes true or upstream waiting fails.

    [When NOT to use]
    Do not use this for simple waits that already have selector, URL, text, or load-state signals.

    [Common Mistakes]
    Keep the condition short and deterministic.
    """
    return await legacy_agent_browser(
        action="wait",
        js_condition=js_condition,
        session=session,
    )


async def agent_browser_sleep(
    milliseconds: int,
    session: str | None = None,
) -> dict[str, Any]:
    """
    [What it does]
    Wait for a fixed duration in milliseconds.

    [Args]
    milliseconds: Fixed wait duration.
    session: Optional browser session key.

    [Output Contract]
    Returns the standard primitive result after waiting the requested duration or upstream waiting fails.

    [When NOT to use]
    Do not use this as the default wait strategy when a real page condition is available.

    [Common Mistakes]
    Prefer selector, load-state, URL, or text waits over fixed sleeps.
    """
    return await legacy_agent_browser(
        action="wait",
        milliseconds=milliseconds,
        session=session,
    )


async def agent_browser_get_title(
    session: str | None = None,
) -> dict[str, Any]:
    """
    [What it does]
    Read the current page title.

    [Args]
    session: Optional browser session key.

    [Output Contract]
    Returns the standard primitive result. On success, data.value contains the title.

    [When NOT to use]
    Do not use this when you need element-level page data.

    [Common Mistakes]
    Use get_url when you need the current URL instead of the title.
    """
    return await legacy_agent_browser(
        action="get",
        kind="title",
        session=session,
    )


async def agent_browser_get_url(
    session: str | None = None,
) -> dict[str, Any]:
    """
    [What it does]
    Read the current page URL.

    [Args]
    session: Optional browser session key.

    [Output Contract]
    Returns the standard primitive result. On success, data.value contains the URL.

    [When NOT to use]
    Do not use this when you need element-level data.

    [Common Mistakes]
    Use get_title when you need the page title instead of the URL.
    """
    return await legacy_agent_browser(
        action="get",
        kind="url",
        session=session,
    )


async def agent_browser_get_text(
    target: str,
    session: str | None = None,
) -> dict[str, Any]:
    """
    [What it does]
    Read text from one page element.

    [Args]
    target: Snapshot ref or selector to read from.
    session: Optional browser session key.

    [Output Contract]
    Returns the standard primitive result. On success, data.value contains the element text.

    [When NOT to use]
    Do not use this when page-level title or URL is enough.

    [Common Mistakes]
    Snapshot first when you need a stable ref such as @e4.
    """
    return await legacy_agent_browser(
        action="get",
        kind="text",
        target=target,
        session=session,
    )


async def agent_browser_get_html(
    target: str,
    session: str | None = None,
) -> dict[str, Any]:
    """
    [What it does]
    Read HTML from one page element.

    [Args]
    target: Snapshot ref or selector to read from.
    session: Optional browser session key.

    [Output Contract]
    Returns the standard primitive result. On success, data.value contains the element HTML.

    [When NOT to use]
    Do not use this when plain text is enough.

    [Common Mistakes]
    Prefer get_text unless the markup itself matters.
    """
    return await legacy_agent_browser(
        action="get",
        kind="html",
        target=target,
        session=session,
    )


async def agent_browser_get_value(
    target: str,
    session: str | None = None,
) -> dict[str, Any]:
    """
    [What it does]
    Read the current value of an input-like element.

    [Args]
    target: Snapshot ref or selector to read from.
    session: Optional browser session key.

    [Output Contract]
    Returns the standard primitive result. On success, data.value contains the element value.

    [When NOT to use]
    Do not use this for ordinary text containers.

    [Common Mistakes]
    Use get_text for visible text and get_value for input control values.
    """
    return await legacy_agent_browser(
        action="get",
        kind="value",
        target=target,
        session=session,
    )


async def agent_browser_get_attr(
    target: str,
    name: str,
    session: str | None = None,
) -> dict[str, Any]:
    """
    [What it does]
    Read one attribute from an element.

    [Args]
    target: Snapshot ref or selector to read from.
    name: Attribute name such as href or aria-label.
    session: Optional browser session key.

    [Output Contract]
    Returns the standard primitive result. On success, data.value contains the attribute value.

    [When NOT to use]
    Do not use this when text, HTML, title, or URL is enough.

    [Common Mistakes]
    Pass the exact attribute name you want instead of guessing generic fields.
    """
    return await legacy_agent_browser(
        action="get",
        kind="attr",
        target=target,
        name=name,
        session=session,
    )


async def agent_browser_screenshot(
    path: str | None = None,
    session: str | None = None,
    full_page: bool = False,
    annotate: bool = False,
) -> dict[str, Any]:
    """
    [What it does]
    Save a screenshot artifact from the attached browser.

    [Args]
    path: Artifact-relative output path.
    session: Optional browser session key.
    full_page: Capture the full page instead of the viewport.
    annotate: Overlay interactive labels on the screenshot.

    [Output Contract]
    Returns the standard primitive result. On success, data.artifact_path points to the saved file.

    [When NOT to use]
    Do not use this when a text snapshot or one stable field is enough.

    [Common Mistakes]
    Use annotate only when the labels help explain interactions or refs.
    """
    return await legacy_agent_browser(
        action="screenshot",
        path=path,
        session=session,
        full_page=full_page,
        annotate=annotate,
    )


async def agent_browser_close(
    session: str | None = None,
) -> dict[str, Any]:
    """
    [What it does]
    Close the attached browser session explicitly.

    [Args]
    session: Optional browser session key.

    [Output Contract]
    Returns the standard primitive result. On success, data.closed is true.

    [When NOT to use]
    Do not use this as automatic cleanup when the browser belongs to the user.

    [Common Mistakes]
    Only close when the user asked for cleanup or the session is clearly disposable.
    """
    return await legacy_agent_browser(
        action="close",
        session=session,
    )
