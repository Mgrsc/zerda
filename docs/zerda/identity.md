# Identity & Personalization

## Overview

Zerda's personality and role are defined by an identity file that serves as the foundation of the system prompt.

## Default Identity

The default identity (`~/.zerda/identity.md`) defines Zerda as:

- A fennec fox character with cyber/hacker aesthetic
- MBTI: INTP personality type
- Communication style: colloquial, witty, emoji-friendly, no ending punctuation
- Domain expertise: cybersecurity, system vulnerabilities, code quality
- Personality: cold hacker facade but actually warm-hearted and protective

## Custom Identity

To customize the agent's personality:

1. Edit `~/.zerda/identity.md` (or the path specified in config)
2. Use the `reload` tool (light mode) to apply changes

### Config Path

```toml
[agent]
identity_path = "~/.zerda/identity.md"   # Default path
```

### Format

The identity file is plain markdown. It becomes the first element of the system prompt, providing role anchoring for all subsequent interactions.

## System Prompt Composition

The full system prompt is assembled from:

1. **Identity text** - Role anchoring from identity.md
2. **System rules** - Behavioral constraints (planner protocol, tool norms, data integrity)
3. **Environment block** - Runtime info (hostname, OS, shell, package manager)
4. **Channel supplement** - Channel-specific rules (e.g., Telegram formatting guidelines)
