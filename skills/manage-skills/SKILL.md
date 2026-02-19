---
name: manage-skills
description: |
  Search, install, uninstall, or create agent skills from the skills.sh registry
---

# Manage Skills

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
SKILLS_DIR="${SKILLS_DIR:-$HOME/.zerda/skills}"
TARGET="$SKILLS_DIR/$SKILL"

mkdir -p "$TARGET"
cd $(mktemp -d)
git clone --depth 1 --filter=blob:none --sparse "https://github.com/$OWNER/$REPO.git" .
git sparse-checkout set "skills/$SKILL" 2>/dev/null || git sparse-checkout set "$SKILL"
cp -r skills/"$SKILL"/* "$TARGET"/ 2>/dev/null || cp -r "$SKILL"/* "$TARGET"/
cd - > /dev/null
```

Default skill directory is `~/.zerda/skills`. If `agent.memory_dir` is customized in `zerda.toml`, set `SKILLS_DIR` to `<resolve_path(agent.memory_dir)>/skills` first.

After the files are in place, call the `reload` tool with `mode=light` to activate the new skill. Wait for the system confirmation message before informing the user.

### Well-Known Skill Sources

- `anthropics/skills` — pdf, docx, pptx, xlsx, skill-creator, mcp-builder, frontend-design, etc.
- `vercel-labs/agent-skills` — Vercel/React/Next.js best practices
- `ComposioHQ/awesome-claude-skills` — community curated collection

## Uninstall

```bash
SKILLS_DIR="${SKILLS_DIR:-$HOME/.zerda/skills}"
rm -rf "$SKILLS_DIR/<skill-name>"
```

If `agent.memory_dir` is customized, set `SKILLS_DIR` to `<resolve_path(agent.memory_dir)>/skills` first.

After removing the directory, call the `reload` tool with `mode=light` to update the skill index.

## Create

When the user wants to create a new skill, read the comprehensive guide at [create/guide.md](create/guide.md) for the full SKILL.md specification, frontmatter fields, argument substitution syntax, and examples.

Skill storage locations:

| Path |
|------|
| `<resolve_path(agent.memory_dir)>/skills/<skill-name>/SKILL.md` |

Default path: `~/.zerda/skills/<skill-name>/SKILL.md`.
