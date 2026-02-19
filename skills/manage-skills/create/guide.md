# Skill Creation Guide

## SKILL.md Structure

Every skill is a directory containing a `SKILL.md` file with two parts:

1. **YAML frontmatter** between `---` markers
2. **Markdown body** with instructions

```yaml
---
name: my-skill
description: |
  What this skill does and when to use it
---

Your skill instructions here...
```

## Frontmatter Fields

All fields are optional. Only `description` is recommended.

| Field | Description |
|-------|-------------|
| `name` | Display name and `/slash-command`. Lowercase letters, numbers, hyphens only (max 64 chars). Defaults to directory name. |
| `description` | What the skill does and when to use it. Claude uses this to decide when to load the skill automatically. If omitted, uses first paragraph of markdown content. |
| `argument-hint` | Hint shown during autocomplete. Example: `[issue-number]` or `[filename] [format]`. |
| `disable-model-invocation` | Set `true` to prevent Claude from auto-loading this skill. User must invoke manually via `/name`. Default: `false`. |
| `user-invocable` | Set `false` to hide from `/` menu. Use for background knowledge. Default: `true`. |
| `allowed-tools` | Tools Claude can use without permission when this skill is active. Comma-separated. |
| `model` | Model to use when this skill is active. |
| `context` | Set to `fork` to run in a forked subagent context. |
| `agent` | Subagent type when `context: fork` is set. Options: `Explore`, `Plan`, `general-purpose`, or custom agent name. |
| `hooks` | Hooks scoped to this skill's lifecycle. |

### Invocation Control

| Frontmatter | User can invoke | Claude can invoke |
|-------------|-----------------|-------------------|
| (default) | Yes | Yes |
| `disable-model-invocation: true` | Yes | No |
| `user-invocable: false` | No | Yes |

## Storage Locations

| Scope | Path |
|-------|------|
| Personal | `~/.zerda/skills/<skill-name>/SKILL.md` |
| Project | `.claude/skills/<skill-name>/SKILL.md` |

When skills share the same name across levels, higher-priority locations win.

## Argument Substitution

Skills support string substitution for dynamic values:

| Variable | Description |
|----------|-------------|
| `$ARGUMENTS` | All arguments passed when invoking the skill. If not present in content, arguments are appended as `ARGUMENTS: <value>`. |
| `$ARGUMENTS[N]` | Access a specific argument by 0-based index. |
| `$N` | Shorthand for `$ARGUMENTS[N]` (`$0` = first, `$1` = second). |
| `${CLAUDE_SESSION_ID}` | Current session ID. |

Example:

```yaml
---
name: fix-issue
description: Fix a GitHub issue
disable-model-invocation: true
---

Fix GitHub issue $ARGUMENTS following our coding standards.
```

Invoke: `/fix-issue 123` → Claude receives "Fix GitHub issue 123 following our coding standards."

Multi-argument example:

```yaml
---
name: migrate-component
description: Migrate a component between frameworks
---

Migrate the $0 component from $1 to $2.
Preserve all existing behavior and tests.
```

Invoke: `/migrate-component SearchBar React Vue`

## Directory Structure

Skills can include supporting files alongside `SKILL.md`:

```
my-skill/
├── SKILL.md           # Main instructions (required)
├── template.md        # Template for Claude to fill in
├── examples/
│   └── sample.md      # Example output
└── scripts/
    └── validate.sh    # Script Claude can execute
```

Reference supporting files from `SKILL.md` so Claude knows when to load them:

```markdown
## Additional resources
- For complete API details, see [reference.md](reference.md)
- For usage examples, see [examples.md](examples.md)
```

Keep `SKILL.md` under 500 lines. Move detailed reference material to separate files.

## Dynamic Context Injection

The `` !`command` `` syntax runs shell commands before the skill content is sent to Claude. The command output replaces the placeholder.

```yaml
---
name: pr-summary
description: Summarize changes in a pull request
context: fork
agent: Explore
---

## Pull request context
- PR diff: !`gh pr diff`
- PR comments: !`gh pr view --comments`
- Changed files: !`gh pr diff --name-only`

Summarize this pull request...
```

Each `` !`command` `` executes immediately (before Claude sees anything), and the output replaces the placeholder in the skill content.

## Subagent Mode (`context: fork`)

Add `context: fork` to run the skill in an isolated subagent. The skill content becomes the prompt that drives the subagent — it won't have access to conversation history.

Only use `context: fork` for skills with explicit task instructions. Guidelines-only skills (without an actionable task) will return without meaningful output.

```yaml
---
name: deep-research
description: Research a topic thoroughly
context: fork
agent: Explore
---

Research $ARGUMENTS thoroughly:

1. Find relevant files using Glob and Grep
2. Read and analyze the code
3. Summarize findings with specific file references
```

## Complete Example

```yaml
---
name: deploy
description: |
  Deploy the application to production. Use when the user wants to ship
  changes to production or staging environments.
context: fork
disable-model-invocation: true
allowed-tools: Bash(*)
---

Deploy $ARGUMENTS to production:

1. Run the test suite
2. Build the application
3. Push to the deployment target
4. Verify the deployment succeeded
```
