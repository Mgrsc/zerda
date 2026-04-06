# Identity

## Overview

Zerda reads a markdown identity file and prepends it to the system prompt.

## Config

```toml
[agent]
identity_path = "~/.zerda/identity.md"
```

## Behavior

- The identity file is loaded at startup.
- It becomes the first system-prompt segment.
- Changes take effect after a process restart.
