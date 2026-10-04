---
name: agentsync
description: AgentSync config — AGENTS.md, rules, skills, commands, subagents, settings, hooks, MCP, syncing .ai/src to Claude, Codex, Cursor, OpenCode. Use when editing them or asking why a skill did not load.
---

# Working with AgentSync

Create and maintain AI agent instructions in the AgentSync format.

## Structure

```
.ai/src/                        # Source of truth. Edit ONLY here.
├── AGENTS.md                   # Agent identity: role, approach, principles
├── rules/                      # Always-on constraints (one file per topic)
│   ├── core.md
│   └── testing.md
├── skills/                     # On-demand recipes (one directory per skill)
│   ├── deploy/
│   │   └── SKILL.md
│   └── flutter/                #   optional category (no SKILL.md of its own)
│       └── bloc/
│           └── SKILL.md
├── commands/                   # Custom slash commands (.md files)
│   ├── review.md
│   └── fix-issue.md
├── agents/                     # Subagent personas (.md files)
│   └── code-reviewer.md
├── mcp.json                    # Shared MCP servers for compatible targets
└── tools/                      # Per-tool config and overrides
    ├── claude.yaml             #   tool config: dest paths, format options
    └── claude/                 #   per-tool payload overrides (opt-in)
        ├── settings.json       #     permissions / settings override
        ├── hooks.json          #     hooks override
        └── mcp.json            #     per-tool MCP override (shadows mcp.json above)
```

After editing, run `agentsync sync` to distribute to all tools.

Group many skills into category directories — a directory without `SKILL.md` is a category, up to four levels deep. No tool reads categories the same way (Claude Code, VS Code Copilot, and Gemini CLI skip nested skills), so sync lands every skill flat at `<dest>/<name>/`. Leaf names stay unique across categories: two skills sharing a name stop sync. Filter a whole category with a path glob — `exclude: cloudflare/*` in a tool's `targets.skills` — and inspect the layout with `agentsync skills check`. A shipped skill moved into a category still receives `agentsync refresh` updates there.

Settings, hooks, and per-tool MCP are overrides: they only exist once you opt in (`agentsync enable`, `agentsync customize`, `agentsync add mcp`). When absent, AgentSync falls back to its shipped base templates. The flat `settings/`, `mcp/`, and `hooks/` directories from older layouts still work but are deprecated — preview their move with `agentsync migrate --legacy` and apply it with `agentsync migrate --apply`.

## Scaffolding new content

Use `agentsync add <kind> <name>` to create a new file with the correct frontmatter and placement:

- `agentsync add rule <name>` — creates `.ai/src/rules/<name>.md`
- `agentsync add skill <name> [--category <path>]` — creates `.ai/src/skills/[<category>/]<name>/SKILL.md`
- `agentsync add command <name>` — creates `.ai/src/commands/<name>.md`
- `agentsync add subagent <name>` — creates `.ai/src/agents/<name>.md`
- `agentsync add mcp <name> (--command CMD [--args '…'] [--env K=V,…] | --url URL)` — adds a server to the shared `.ai/src/mcp.json`

The command refuses to overwrite existing files; pass `--force` (or `-f`) to replace them. Names must contain only letters, digits, hyphens, and underscores — no path separators, no `..`, no leading `.` or `-`. `agentsync skills list` / `show <name>` / `check` inspect the effective skills and their metadata without changing sync output.

## Writing AGENTS.md

The agent's identity. Every sentence should change behavior.

- **Be specific** — "Senior React/TypeScript Engineer" not "software engineer".
- **Include the stack** — The agent needs to know what it's working with.
- **Actionable principles** — "Prefer composition over inheritance" not "Write good code".
- **Boundaries** — Call out hard limits as required behavior ("treat `db/migrations/` as append-only", "every endpoint goes through `auth.requireUser`"). Phrase boundaries as required behavior wherever practical, and grant the safe workflows explicitly ("the local tests use disposable fixtures and have no production access — run them and rerun affected tests without asking at each step"): ask-first language written to rein in an older model reads to a current one (GPT-6 Astra in OpenAI's guidance) as a stop sign, and it stops where you wanted it to continue. Point to docs contextually ("use `database.md` for schema changes, `deployment.md` when preparing a release"); a rule that reads three docs before every edit spends that context on a typo fix too. When one person works across risk tiers, the file carries this repo's tier — "no unattended merges", the load-bearing paths named, a plan before edits — so the constraint lives in the file rather than in your memory of which mode you are in.
- **Start from a blank file, not a generated draft.** The model reads the code, so a directory tour or stack summary restates what it already infers and costs attention on every turn. A line earns its place by carrying what the code cannot show — a tooling gotcha, a non-obvious convention, why `legacy/` stays — and by tracing to something that went wrong; remove it once the underlying problem is fixed. Two studies agree: a developer-written `AGENTS.md` cut agent runtime ~29% and output tokens ~17% across 124 real PRs ([Lulla et al.](https://arxiv.org/abs/2601.20404)), while an ETH benchmark ([Gloaguen et al.](https://www.sri.inf.ethz.ch/publications/gloaguen2026agentsmd)) found context files adding over 20% inference cost with no success gain and concluded that a human-written file should carry only minimal requirements.
- 40–70 lines. No generic filler.

## Writing Rules

Always-on constraints. One file per topic in `.ai/src/rules/`.

- **One concern per file** — `testing.md`, `security.md`. Not `everything.md`.
- **Imperative and specific** — "Use `snake_case` for DB columns" not "Follow naming conventions".
- **Constraints, not tutorials** — Tell the agent what behavior to produce. Skip concept explanations the model already knows.
- **Prefer positive instructions** — Per Anthropic's prompt-engineering guidance, "respond in flowing prose" works better than a prohibition. Phrase rules as what to do; express hard boundaries as required behavior.
- **20–50 lines per file** — If it grows beyond that, split by topic. Multiple small focused files beat one large catch-all.
- **Always-on by default — scope domain rules with `paths:`** — A rule with no frontmatter loads on every task. For a domain rule (state, routing, data…), add `paths:` frontmatter (a list of globs) so it loads only when matching files are touched. AgentSync translates `paths:` to each tool's native trigger (Claude `paths:`, Cursor `globs`+`alwaysApply:false`, Copilot `applyTo`, Devin Desktop/Antigravity `trigger: glob`). Keep the always-on set lean — a wall of always-on rules dilutes attention and the agent starts ignoring individual instructions. Before adding an always-on line, apply the removal test — _would deleting this let a likely mistake through?_ If not, it's noise; cut it or scope it with `paths:`.
- **Add a line only after a real failure, and after cheaper remedies** — first fix the code or the misused API behind the slip; then let a hook, lint, or test enforce the behavior deterministically (_Settings, permissions, and hooks_ below); add a rule line last. The failure it prevents justifies the line, and the line leaves when a code fix or a stronger model makes it redundant — the addition test that pairs with the removal test above.
- **Compose rules by level in a workspace** — put broad infra/governance constraints in the parent `.ai/src/rules/` (inherited via `shared:` — `references/workspaces-and-profiles.md`) and keep feature-specific rules scoped at the leaf project. Layering this way keeps each level's always-on set minimal instead of one project carrying every rule.

## Writing Skills — The Most Important Part

Skills are the highest-leverage configuration. AgentSync skills follow the open [agentskills.io](https://agentskills.io) format — a portable standard supported by Claude Code, Codex, Cursor, Copilot, Gemini CLI, OpenCode, and ~30 other agents. Validate with `skills-ref validate <path>`.

The **description is the trigger** — vague descriptions never activate. Open with the domain keywords the user would say (`Flutter authentication — login, logout, session restore…`), then the "Use when…" conditions; be pushy about phrasings (list cases where the user doesn't name the domain) and keyword-rich, but keep the domain itself narrow: as short as it can be while the trigger is unambiguous. OpenAI's example for GPT-6 Astra — "Use when adding or changing a migration, or reviewing its rollout", not "Use when working with databases, queries, models, or persistence" — the broad form loads the skill whenever the model touches a database, and an over-emphasised description loads instructions that don't help the task. Hosts shorten descriptions once the skill listing is over budget — Codex shortens every description once many skills are installed — so the first 50 characters carry the match on their own, and "Use this skill when" spends them on nothing. Hard limit: 1024 chars.

The **directory layout** is `SKILL.md` + optional `references/` (load-on-demand docs), `scripts/` (executable code), `assets/` (templates). Keep `SKILL.md` ≤ 500 lines / ≤ 5000 tokens; move detail behind explicit triggers ("read `references/maintenance.md` when investigating override drift").

**When creating or editing a skill in `.ai/src/skills/<name>/`, read [`references/writing-skills.md`](references/writing-skills.md)** — it covers the full agentskills.io spec, frontmatter constraints, structure templates, calibration principles (procedures-over-declarations, defaults-not-menus, match-specificity-to-fragility), reusable patterns (Gotchas, Templates, Checklists, Validation loops, Plan-validate-execute), and the iteration loop with evals.

**When a skill is high-stakes, under-triggers, or you need to prove a new version beats the old, read [`references/evaluating-skills.md`](references/evaluating-skills.md)** — the measurement half of the loop: with-skill-vs-baseline runs, discriminating assertions, reading the benchmark, blind comparison, and trigger-query optimization.

**Rule of three:** wait for three manual repetitions of a workflow before turning it into a skill.

## Writing Commands

Custom slash commands. Each `.md` file in `.ai/src/commands/` becomes a command (e.g., `review.md` → `/project:review`).

```markdown
---
description: What this command does (shown in command list)
argument-hint: "<optional-arg>"
---

[Prompt content with instructions for the AI.]
```

Key features:

- A dollar sign followed by `ARGUMENTS` — replaced with text after the command name.
- An exclamation mark directly before an inline-code command — runs the command and embeds its output into the prompt.
- Keep commands focused — one workflow per command.
- Avoid `: ` (colon-space) inside an unquoted `description:` (see Gotchas).
- Good commands: `changelog`, `fix-issue`, `deploy`, `migrate`.

## Writing Agents (Subagent Personas)

Specialized AI personas in `.ai/src/agents/`. Each `.md` file defines an agent with its own system prompt and tool restrictions.

```markdown
---
name: code-reviewer
description: Use proactively when reviewing PRs or validating implementations — expert code reviewer.
model: sonnet # Cheaper model for focused tasks
tools: [Read, Grep, Glob] # Restrict to read-only tools
---

You are a senior code reviewer...
```

Guidelines:

- Restrict `tools` to what the agent actually needs. Read-only agents stay read-only.
- Use `model: sonnet` or `model: haiku` for focused tasks to save cost.
- Create agents only for distinct specializations — workflows that fit an existing skill stay as skills.

## MCP, inline options, and new tools

**Read [`references/mcp-and-tool-targets.md`](references/mcp-and-tool-targets.md) when registering an MCP server, when a tool has no native rules / skills / commands directory, or when onboarding a new tool.** It covers the shared `.ai/src/mcp.json`, the per-host routing that decides whether a server is actually live (Claude's project-only `.mcp.json`, Codex's `config.toml`, OpenCode's composed config), key ownership in files a tool also writes, the `inline_into_agents` / `as_skills` / `prepend_agents` / `legacy_dest` options with the tools that use each, and the steps to add a target.

## Settings, permissions, and hooks

A rule in `AGENTS.md`/`rules/` is **advisory** — context the model can ignore under pressure; a hook is **enforced** — the harness runs it every time. Reach for a hook when something _must_ happen, not merely _should_.

**Read [`references/settings-hooks-and-harness.md`](references/settings-hooks-and-harness.md) when overriding a tool's settings or permissions, writing a hook, or turning an agent mistake into a harness change.** It covers `enable` / `customize` overrides, three-valued `allow` / `ask` / `deny` tiered by consequence, Claude Code's hook events (`SessionStart`, `PreToolUse` blocking on exit 2, `PostToolUse`, `Stop`), the guard hook that keeps edits in `.ai/src/`, and the slip → missing component → smallest fix → stacked-layers loop.

## Maintenance

**Read [`references/maintenance.md`](references/maintenance.md) when running `agentsync update`, `resolve`, `simplify`, `check`, `doctor`, `rollback`, `migrate`, or `upgrade-config`, or when investigating stale-override or upstream-drift problems.** It covers `.ai/.pending-resolutions.yaml`, `--strict` in CI, simplify's dry-run and idempotency, backups, rollback and retention, version pins, where outputs live, and the recommended cadence.

**Recovering a directly-edited generated file.** `agentsync sync` records every generated file in `.ai/.sync-manifest` (SHA-256), so a generated file edited by hand — or one a tool writes into out of band — makes the next sync abort instead of overwriting it. Run `agentsync adopt <file>` (or `adopt --all`) to promote the current content into `.ai/src/`, then sync again. `adopt` refuses to run non-interactively without `--yes`.

**Read [`references/workspaces-and-profiles.md`](references/workspaces-and-profiles.md) when running `agentsync refresh`, when a parent workspace shares content with sub-projects, or when a tool needs a second config home.** It covers the three-way template diff and its outcome classes, `template_overrides`, declarative `shared:` inheritance with `dedupe` and `doctor` cross-project detection, the `category: governance` marker, and the profile overlay layout.

## Who owns which file

Two layers, and the difference decides whether an upgrade reaches you:

- **Engine-owned** — this skill. It ships inside the `agentsync` binary and is resolved at sync time, so an engine upgrade updates it in every project. To add to it and keep the updates, create an `agentsync/` directory with no `SKILL.md` (at the skills root or in any category): its files join the engine's, a file at the same path replaces the engine's, and its `SKILL.append.md` is appended to this `SKILL.md` at sync. A directory with its own `SKILL.md` replaces this skill and stops the updates; `base_skills: false` drops it.
- **Project-owned** — everything else under `.ai/src/`. Scaffolded once by `init`, updated only when you accept it via `agentsync refresh`, never overwritten by an upgrade.

`format:` in `agent_sync.yaml` records which migrations the project has been through. When the engine ships a newer revision, the next command says so; `agentsync migrate` previews it and `migrate --apply` performs it.

## Gotchas

- Edit files in `.ai/src/`. Generated directories (`.claude/`, `.cursor/`, etc.) are sync output, not edit targets. A file you add by hand to a generated directory is preserved with a warning but never managed; move it into `.ai/src/`, or run `agentsync sync --force` to prune it.
- Run `agentsync sync` after every change to distribute updates.
- Frontmatter is YAML: a `: ` (colon + space) inside an unquoted `description` invalidates the file and the consuming tool **silently skips the skill at load** — rephrase with `—` and re-validate (`skills-ref validate <path>`) after every frontmatter edit.
- Tool-specific frontmatter fields (like `context: fork`) are passed through as-is — agentsync doesn't validate them.
- Keep skill triggers mutually exclusive. When two skills could fire on the same task, merge them or sharpen their descriptions.
- Native commands land in Claude, Cursor, Copilot, Gemini (as TOML), Junie, Cline, Devin Desktop, Antigravity, and OpenCode. Tools without a command surface get a conversion: Codex, Kimi Code, and Kiro emit generated skills under `command-*/`; Amazon Q and Zed inline a `## Commands` index into their agents file.
- Native subagents land in Claude, Copilot, Cursor, Gemini, and Junie. Codex receives them converted to TOML, Amazon Q as custom-agent JSON, OpenCode as safe Markdown with translated permissions, and Kiro as Markdown agents with its tool tags. Cline, Kimi Code, Zed, Devin Desktop, and Antigravity have no custom subagent surface, so they get none.
- The shared `.ai/src/mcp.json` reaches every compatible MCP target, but Claude Code reads it only at project scope — `references/mcp-and-tool-targets.md` has the per-host route.
- AgentSync owns `.opencode/plugins/agentsync.ts`, not sibling OpenCode plugins, custom tools, themes, TUI preferences, or credentials. Kimi custom agents and project hooks are unavailable; leave Kimi's global config untouched.
- A third-party or plugin skill is executable trust, not just docs — its bundled scripts run with your permissions and its instructions steer the agent. Audit the SKILL.md and every bundled file before installing one; skills that fetch from an external URL at runtime are the highest risk. Treat installing a skill like adding a dependency: private data, untrusted content, and an outbound path in one place make it as dangerous as an unvetted MCP server.
- Popularity is not quality. A widely-starred third-party skill can still raise token cost _and_ worsen results — most shared skills are never rigorously evaluated, and a star count only measures reach, not effect. Before adopting one, benchmark it with-skill-vs-baseline (`references/evaluating-skills.md`) and prefer skills whose authors publish a real evaluation over ones that merely _claim_ to make the agent better.
