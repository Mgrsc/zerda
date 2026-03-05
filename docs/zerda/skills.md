# Skills System

## Overview

Skills are markdown-based instruction packages that extend Zerda's capabilities. They provide domain-specific knowledge and procedures that the agent can activate on demand.

## Directory Structure

Skills are stored in `~/.zerda/skills/`:

```
~/.zerda/skills/
├── skill-name-1/
│   └── SKILL.md
├── skill-name-2/
│   └── SKILL.md
│   └── supporting-file.txt
```

Each skill must have a `SKILL.md` file as its entry point.

## SKILL.md Format

```markdown
---
name: skill-name
description: Brief description shown in skill index
---

Skill instructions go here. The agent reads this entire content
when the skill is activated.

Use $ARGUMENTS to reference arguments passed when activating the skill.
```

The YAML frontmatter is required with `name` and `description` fields.

## How Skills Work

1. On startup (and after `reload`), Zerda scans the skills directory
2. A skills index (names + descriptions) is injected into the system prompt
3. The agent can activate a skill via the `skill` tool when relevant
4. The full SKILL.md content is returned to the agent as tool output
5. `$ARGUMENTS` placeholders are replaced with any arguments provided

## Hot Reloading

Skills are reloaded when the `reload` tool is called with `mode='light'`. No restart needed.

## Well-Known Skill Sources

- `anthropics/skills` - PDF, DOCX, PPTX, XLSX processing, skill-creator, mcp-builder
- `vercel-labs/agent-skills` - Vercel/React/Next.js best practices
- `ComposioHQ/awesome-claude-skills` - Community curated collection

## Creating a Skill

1. Create a directory under `~/.zerda/skills/` with your skill name
2. Add a `SKILL.md` file with YAML frontmatter (`name`, `description`)
3. Write instructions in the body
4. Call `reload` (light mode) to load the new skill
