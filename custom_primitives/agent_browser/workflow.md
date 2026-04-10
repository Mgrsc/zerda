# agent_browser workflow

Use this workflow when `agent_browser` is unfamiliar, when setup might be missing, or when you need the recommended browser loop

Typical discovery order:

1. `help("agent_browser")`
2. `help("agent_browser.connect_cdp")`
3. `agent_browser.get_workflow()`

## install

Install before first use.
npm install agent-browser
agent-browser install

## Purpose

`agent_browser` is for attaching to a browser that the user already exposed through CDP and then performing an interactive loop of snapshot, action, wait, and re-snapshot

This is usually **not** a launch-and-close tool

- The browser often belongs to the user
- The user may want to keep it open after the task
- Do not call `close` unless the user explicitly asks for it or the session is clearly disposable

## Default loop

1. Read this workflow if you are not already confident about setup or the interaction loop
2. Connect to the user-provided CDP target with `agent_browser.connect_cdp(target="...")`
3. Reuse the same browser session for later actions. If the runtime stores a default session for the conversation, later calls may omit `session`
4. Open a target URL with `agent_browser.open(url="...")` only when navigation is needed
5. Capture interactive refs with `agent_browser.snapshot()`
6. Use interaction methods with refs such as `@e1` to click, fill, select, press, hover, or scroll
7. Use one of the wait methods when the page is loading, redirecting, or rendering slowly
8. Call `agent_browser.snapshot()` again after navigation or meaningful DOM changes
9. Use read methods such as `agent_browser.get_title()`, `agent_browser.get_text(target="...")`, or `agent_browser.get_attr(...)` to read stable data
10. Use `agent_browser.screenshot(path="...")` when a visual artifact is needed
11. Keep repeating `snapshot -> interact -> wait -> snapshot` as needed

## Important rules

- Prefer the CDP target provided by the user instead of auto-discovery
- Do not guess undocumented parameters or method names. If something is unclear, inspect `help("agent_browser")` or `help("agent_browser.method")` first
- Refs like `@e1` are temporary and may become invalid after page changes
- After click, submit, redirect, modal changes, or lazy rendering, take a new snapshot before reusing old refs
- Prefer refs from snapshot output over guessed selectors when possible
- Prefer waiting on real conditions like load state, URL, selector, or text instead of fixed sleeps
- `agent_browser.close()` is optional and should usually be treated as manual cleanup, not automatic task completion

## Recommended patterns

### Basic attach and inspect

1. `help("agent_browser")`
2. `agent_browser.get_workflow()`
3. `agent_browser.connect_cdp(target="9222")`
4. `agent_browser.snapshot()`

### Navigate and interact

1. `agent_browser.open(url="https://example.com")`
2. `agent_browser.wait_for_load(load_state="networkidle")`
3. `agent_browser.snapshot()`
4. `agent_browser.click(target="@e1")`
5. `agent_browser.wait_for_load(load_state="networkidle")`
6. `agent_browser.snapshot()`

### Form flow

1. `agent_browser.snapshot()`
2. `agent_browser.fill(target="@e1", text="user@example.com")`
3. `agent_browser.fill(target="@e2", text="password123")`
4. `agent_browser.click(target="@e3")`
5. `agent_browser.wait_for_load(load_state="networkidle")`
6. `agent_browser.snapshot()`

### Read and verify

1. `agent_browser.get_title()`
2. `agent_browser.get_text(target="@e4")`
3. `agent_browser.screenshot(path="browser/result.png")`

## `get` contract

- `kind="title"` reads the current page title and does not require `target`
- `kind="url"` reads the current page URL and does not require `target`
- `kind="text"` reads element text and requires `target`
- `kind="html"` reads element HTML and requires `target`
- `kind="value"` reads an element value and requires `target`
- `kind="attr"` reads one element attribute and requires both `target` and `name`

Examples:

1. `agent_browser.get_title()`
2. `agent_browser.get_url()`
3. `agent_browser.get_text(target="@e4")`
4. `agent_browser.get_attr(target="@e4", name="href")`

## Action notes

- `connect_cdp`: attach to an existing browser through a port, HTTP endpoint, or WebSocket endpoint
- `open`: navigate the attached browser to a new URL
- `snapshot`: inspect the current page and obtain refs
- `click` / `fill` / `type` / `select` / `press` / `hover` / `scroll`: perform one interaction
- `wait_for_*`: wait for the page to settle or for a condition to become true
- `get_*`: read text, HTML, title, URL, value, or attributes
- `screenshot`: write an artifact file
- `close`: only for explicit cleanup
