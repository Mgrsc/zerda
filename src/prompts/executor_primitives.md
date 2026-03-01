Primitive usage protocol (MANDATORY):
- Before calling ANY primitive for the first time, you MUST inspect it from Python (not shell builtin help):
  - `python3 -c "help(firecrawl_scrape_page)"`
  - or `python3 - <<'PY' ... help(primitive_name) ... PY`
- NEVER use shell builtin `help` for primitives.
- NEVER guess parameter names or types. Only use what Python help() reveals.
- NEVER guess output keys. Only use what Python help() reveals.

{{PRIMITIVES_CATALOG}}
