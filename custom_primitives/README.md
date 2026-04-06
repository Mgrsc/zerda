# Custom Primitives

This directory stores all non-core Python primitives.

- Package root: `custom_primitives/`
- Runtime discovery source: `custom`
- Runtime execution: PTC Python bootstrap imports `custom_primitives.catalog` from the Zerda working directory
- `custom_primitives/catalog.py` is the registration point and source of truth for custom primitive exposure
- Primitive files may be grouped into subpackages such as `custom_primitives/agent_browser/`, `custom_primitives/firecrawl/`, and `custom_primitives/scrapling/`
- Public prompt exposure should stay top-level and minimal. Complex families such as `agent_browser` are exposed as one namespace name in `<PTC_AVALIABLE_PRIMITIVES>`, while `help("agent_browser")` reveals their methods.
- Bundled browser interaction is backed by the external `agent-browser` CLI and is publicly intended to be used as `agent_browser.connect_cdp(...)`, `agent_browser.snapshot(...)`, and related namespace methods.
- `scrapling_fetch_page` is the default page-fetch primitive. It automatically routes `mp.weixin.qq.com` article URLs to a WeChat-specific extractor and `x.com` / `twitter.com` URLs to the stealth fetch path, where tweet-body extraction and light UI-noise filtering are preferred over full-page text.
- `scrapling_fetch_page` depends on `scrapling[fetchers]` being available in the Python runtime and returns `dependency_missing` when that dependency is missing.
- `scrapling_fetch_page` may internally use a browser-backed stealth path for selected dynamic domains and therefore also depends on Playwright browser binaries when that internal path is needed.
