# MCP (Model Context Protocol) Integration

## Overview

Zerda supports MCP servers to extend available tools dynamically. MCP tools appear alongside built-in tools and can be called by the agent.

## Configuration

MCP servers are defined in `mcp.toml`, which must be in the same directory as the active `zerda.toml`.

## Stdio Transport

Local subprocess-based MCP server:

```toml
[[mcp]]
name = "filesystem"
transport = "stdio"
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "/home/user"]

[mcp.env]
NODE_ENV = "production"
API_KEY = "${MY_API_KEY}"
```

- `command`: Executable to run
- `args`: Command-line arguments
- `env`: Environment variables for the subprocess (supports `${VAR}` expansion)

Note: MCP subprocesses do NOT auto-inherit the parent environment. Only explicitly defined `env` vars are passed.

## Streamable HTTP Transport

Remote HTTP-based MCP server:

```toml
[[mcp]]
name = "remote-api"
transport = "streamable-http"
url = "https://example.com/mcp"
```

## Lifecycle

- MCP servers are connected at startup
- Changes are applied via `reload` tool with `mode='light'` (no restart needed)
- Failed MCP connections are logged but don't prevent Zerda from starting

## Tool Discovery

MCP tools are automatically discovered from connected servers and registered alongside built-in tools. They appear in the `/status` output under the tools count.
