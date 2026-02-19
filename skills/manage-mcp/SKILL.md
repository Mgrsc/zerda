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

Read `<config-dir>/mcp.toml` where `<config-dir>` is the directory of the active `zerda.toml`, then display all `[[mcp]]` blocks to the user.

### Add

You MUST complete these steps in order:

1. Read `mcp.toml` with the `read` tool to get current content. If the file does not exist, start with an empty file.
2. Use the `write` tool to write the updated `mcp.toml`, appending the new `[[mcp]]` block. Write the COMPLETE file content.
3. Verify the file was written correctly by reading it back with the `read` tool.
4. Only after confirming the write succeeded, call the `reload` tool with `mode=light` to apply the new MCP connection.

### Remove

1. Read `mcp.toml` with the `read` tool to get current content.
2. Use the `write` tool to write the updated `mcp.toml` with the matching `[[mcp]]` block removed. Write the COMPLETE file content.
3. Verify the file was written correctly by reading it back with the `read` tool.
4. Only after confirming the write succeeded, call the `reload` tool with `mode=light`.

## Important

- NEVER call `reload` before writing the config file. If you reload without writing, nothing changes.
- For MCP-only changes, use `reload` with `mode=light` first. If light reload is rejected, retry with `mode=full`.
- After calling `reload` with `mode=light`, wait for the system completion message before reporting success.
- Validate TOML syntax: arrays use `[]`, strings use `""`, no trailing commas.
- If `reload` returns a validation error, read the config, fix the syntax, write it again, and retry.
- The supervisor automatically rolls back the config if the process crashes repeatedly after a config change.
