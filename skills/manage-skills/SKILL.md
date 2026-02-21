---
name: manage-skills
description: |
  Search, install, uninstall, or create agent skills from the skills.sh registry
---

# Manage Skills

## Path Resolution (MUST)

Before install, uninstall, or create operations, resolve the target skills directory from the active `zerda.toml`.

Resolve the active `zerda.toml` in this order:

1. Explicit `--config` path
2. `$ZERDA_CONFIG`
3. `~/.zerda/zerda.toml`

Path handling rules:

- Always expand `~/` to `$HOME/` before reading or writing files.
- The default fallback is `$HOME/.zerda/zerda.toml` (never `$HOME/zerda.toml`).
- Read `agent.memory_dir` from the active `zerda.toml` and resolve it with `resolve_path` semantics.
- Build skills path strictly as `<resolve_path(agent.memory_dir)>/skills`.
- If `agent.memory_dir` is absent, use default `~/.zerda`, so skills path is `$HOME/.zerda/skills`.
- Do not hardcode container paths or current working directory.

## Search

Query the skills.sh registry API:

```bash
curl -s "https://skills.sh/api/search?q=QUERY&limit=10"
```

Response contains a `skills` array. Each entry has `id` (format: `owner/repo/skill-name`), `name`, `installs`, and `source` (format: `owner/repo`).

View a skill's page at: `https://skills.sh/{owner}/{repo}/{skill-name}`

## Install

A skill is a directory containing a `SKILL.md` file. To install from a GitHub repo:

```bash
OWNER="owner"
REPO="repo"
SKILL="skill-name"
SKILLS_DIR="<resolve_path(agent.memory_dir)>/skills"
TARGET="$SKILLS_DIR/$SKILL"

mkdir -p "$TARGET"
cd $(mktemp -d)
git clone --depth 1 --filter=blob:none --sparse "https://github.com/$OWNER/$REPO.git" .
git sparse-checkout set "skills/$SKILL" 2>/dev/null || git sparse-checkout set "$SKILL"
cp -r skills/"$SKILL"/* "$TARGET"/ 2>/dev/null || cp -r "$SKILL"/* "$TARGET"/
cd - > /dev/null
```

Default skill directory is `~/.zerda/skills`. Always prefer `<resolve_path(agent.memory_dir)>/skills` from the active `zerda.toml`.

After the files are in place, call the `reload` tool with `mode=light` to activate the new skill. Wait for the system confirmation message before informing the user.

### Well-Known Skill Sources

- `anthropics/skills` — pdf, docx, pptx, xlsx, skill-creator, mcp-builder, frontend-design, etc.
- `vercel-labs/agent-skills` — Vercel/React/Next.js best practices
- `ComposioHQ/awesome-claude-skills` — community curated collection

## Uninstall

```bash
SKILLS_DIR="<resolve_path(agent.memory_dir)>/skills"
rm -rf "$SKILLS_DIR/<skill-name>"
```

Always set `SKILLS_DIR` to `<resolve_path(agent.memory_dir)>/skills` from the active `zerda.toml`.

After removing the directory, call the `reload` tool with `mode=light` to update the skill index.

## Create

When the user wants to create a new skill, read the comprehensive guide at [create/guide.md](create/guide.md) for the full SKILL.md specification, frontmatter fields, argument substitution syntax, and examples.

Skill storage locations:

| Path |
|------|
| `<resolve_path(agent.memory_dir)>/skills/<skill-name>/SKILL.md` |

Default path: `~/.zerda/skills/<skill-name>/SKILL.md`.
