---
name: manage-mcp
description: |
  Add, remove, or list MCP server configurations when the user wants to manage MCP integrations
---

# Manage MCP Servers

Use this skill when the user wants to add, remove, or view MCP server configurations.

MCP servers are configured as `[[mcp]]` blocks in a dedicated `mcp.toml` file, located next to the active `zerda.toml`. Do not hardcode a container working directory. If `mcp.toml` does not exist yet, create it. MCP connections are established at process startup, so config changes require reload to take effect.

Resolve the active `zerda.toml` in this order:

1. Explicit `--config` path
2. `$ZERDA_CONFIG`
3. `~/.zerda/zerda.toml`

Path handling rules:

- Always expand `~/` to `$HOME/` before reading or writing files.
- The default fallback is `$HOME/.zerda/zerda.toml` (never `$HOME/zerda.toml`).
- Build MCP path strictly as `<parent_dir_of_active_zerda.toml>/mcp.toml`.
- If both `$HOME/.zerda/zerda.toml` and `$HOME/zerda.toml` exist and no explicit config is provided, prefer `$HOME/.zerda/zerda.toml`.

## Configuration Format

### stdio transport

```toml
[[mcp]]
name = "server-name"
transport = "stdio"
command = "npx"
args = ["-y", "@scope/mcp-server-name", "/path/to/resource"]

[mcp.env]
API_KEY = "sk-xxx"
```

The `[mcp.env]` section is optional. Use it to pass environment variables to the MCP subprocess. API keys and secrets that the MCP server needs MUST go here — the subprocess does not automatically inherit all host environment variables. You can use `${VAR}` syntax to reference variables from the host environment.

### streamable-http transport

```toml
[[mcp]]
name = "server-name"
transport = "streamable-http"
url = "https://example.com/mcp"
```

## Operations

### List

1. Resolve active `zerda.toml` path using the order above, with `~/` expanded to `$HOME/`.
2. Compute `<config-dir>` as its parent directory.
3. Read `<config-dir>/mcp.toml` and display all `[[mcp]]` blocks.

### Add

You MUST complete these steps in order:

1. Read `<config-dir>/mcp.toml` with the `read` tool to get current content. If the file does not exist, start with an empty file.
2. Use the `write` tool to write the updated `<config-dir>/mcp.toml`, appending the new `[[mcp]]` block. Write the COMPLETE file content.
3. Verify the file was written correctly by reading it back with the `read` tool.
4. Only after confirming the write succeeded, call the `reload` tool with `mode=light` to apply the new MCP connection.

### Remove

1. Read `<config-dir>/mcp.toml` with the `read` tool to get current content.
2. Use the `write` tool to write the updated `<config-dir>/mcp.toml` with the matching `[[mcp]]` block removed. Write the COMPLETE file content.
3. Verify the file was written correctly by reading it back with the `read` tool.
4. Only after confirming the write succeeded, call the `reload` tool with `mode=light`.

## Important

- NEVER call `reload` before writing the config file. If you reload without writing, nothing changes.
- For MCP-only changes, use `reload` with `mode=light` first. If light reload is rejected, retry with `mode=full`.
- After calling `reload` with `mode=light`, wait for the system completion message before reporting success.
- Validate TOML syntax: arrays use `[]`, strings use `""`, no trailing commas.
- If `reload` returns a validation error, read the config, fix the syntax, write it again, and retry.
- The supervisor automatically rolls back the config if the process crashes repeatedly after a config change.
