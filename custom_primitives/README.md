# Custom Primitives

This directory stores all non-core Python primitives.

- Package root: `custom_primitives/`
- Runtime discovery source: `custom`
- Runtime execution: PTC Python bootstrap imports `custom_primitives.catalog` from the Zerda working directory
- `custom_primitives/catalog.py` is the registration point and source of truth for custom primitive exposure
- Optional runtime dependencies may be declared per primitive package with `requirements.txt`, for example `custom_primitives/divination/requirements.txt`
- `custom_primitives/run_zerda_with_deps.sh` scans `custom_primitives/**/requirements.txt`, creates a dedicated custom-primitives virtualenv with `uv` using `ZERDA_CUSTOM_PRIMITIVES_PYTHON` or `ZERDA_CUSTOM_PRIMITIVES_PYTHON_VERSION` (default `3.13`), installs merged dependencies into that virtualenv, caches the merged hash under `~/.zerda/custom_primitives/`, prepends the virtualenv to `PATH`, and then starts `zerda`
- Primitive files may be grouped into subpackages such as `custom_primitives/agent_browser/`, `custom_primitives/divination/`, `custom_primitives/firecrawl/`, `custom_primitives/smart_search/`, and `custom_primitives/scrapling/`
- Public prompt exposure should stay top-level and minimal. Complex families such as `agent_browser` are exposed as one namespace name in `<PTC_AVALIABLE_PRIMITIVES>`, while `help("agent_browser")` reveals their methods.
- Bundled browser interaction is backed by the external `agent-browser` CLI and is publicly intended to be used as `agent_browser.connect_cdp(...)`, `agent_browser.snapshot(...)`, and related namespace methods.
- `scrapling_fetch_page` is the default page-fetch primitive. It automatically routes `mp.weixin.qq.com` article URLs to a WeChat-specific extractor and `x.com` / `twitter.com` URLs to the stealth fetch path, where tweet-body extraction and light UI-noise filtering are preferred over full-page text.
- `scrapling_fetch_page` depends on `scrapling[fetchers]` being available in the Python runtime and returns `dependency_missing` when that dependency is missing.
- `scrapling_fetch_page` may internally use a browser-backed stealth path for selected dynamic domains and therefore also depends on Playwright browser binaries when that internal path is needed.
- `get_lunar_info`, `convert_lunar_to_solar`, `get_sizhu_info`, and `calculate_meihua` are bundled divination primitives backed by `sxtwl`.
- `smart_search` calls a non-streaming OpenAI-compatible `chat/completions` endpoint for answer-style information retrieval. Configure `SMART_SEARCH_URL`, `SMART_SEARCH_API_KEY`, and `SMART_SEARCH_MODEL`, or pass those values explicitly to the primitive call.
- Example startup: `custom_primitives/run_zerda_with_deps.sh serve`
