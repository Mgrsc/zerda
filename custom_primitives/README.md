# Custom Primitives

This directory stores all non-core Python primitives.

- Package root: `custom_primitives/`
- Runtime discovery source: `custom`
- Runtime discovery scans `custom_primitives/*/pyproject.toml`
- Runtime execution injects custom primitives as async proxies backed by isolated per-package virtual environments
- Python dependencies are declared in `[project.dependencies]` inside each package manifest
- Zerda syncs custom package environments during startup and through `zerda primitives sync`
- Sync state and isolated environments are cached under `~/.zerda/custom_primitives/packages/` by default
- Primitive files may be grouped into subpackages such as `custom_primitives/agent_browser/`, `custom_primitives/divination/`, `custom_primitives/firecrawl/`, `custom_primitives/smart_search/`, and `custom_primitives/scrapling/`
- Public prompt exposure should stay top-level and minimal. Complex families such as `agent_browser` are exposed as one namespace name in `<PTC_AVALIABLE_PRIMITIVES>`, while `help("agent_browser")` reveals their methods.
- Bundled browser interaction is backed by the external `agent-browser` CLI and is publicly intended to be used as `agent_browser.connect_cdp(...)`, `agent_browser.snapshot(...)`, and related namespace methods.
- `scrapling_fetch_page` is the default page-fetch primitive. It automatically routes `mp.weixin.qq.com` article URLs to a WeChat-specific extractor and `x.com` / `twitter.com` URLs to the stealth fetch path, where tweet-body extraction and light UI-noise filtering are preferred over full-page text.
- `custom_primitives/scrapling/pyproject.toml` declares both `scrapling[fetchers]` and `playwright`, and also requests Chromium installation during sync for stealth fetches.
- `get_lunar_info`, `convert_lunar_to_solar`, `get_sizhu_info`, and `calculate_meihua` are bundled divination primitives backed by `sxtwl`.
- `custom_primitives/divination/pyproject.toml` declares `sxtwl==2.0.7`, so Zerda sync installs it into the divination package environment.
- `smart_search` calls a non-streaming OpenAI-compatible `chat/completions` endpoint for answer-style information retrieval. Configure `SMART_SEARCH_URL`, `SMART_SEARCH_API_KEY`, and `SMART_SEARCH_MODEL`, or pass those values explicitly to the primitive call.
- `agent_browser` does not require Python package dependencies, but its package manifest declares the external `agent-browser` command as a readiness requirement.
- Manual maintenance:
  - `zerda primitives sync`
  - `zerda primitives doctor`
