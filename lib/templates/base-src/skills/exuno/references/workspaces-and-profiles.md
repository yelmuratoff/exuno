# Workspaces, profiles, and template refresh

Three things a multi-project or multi-home setup needs: pulling new shipped template content into an existing project, sharing content between a parent workspace and its sub-projects, and running a second config home for the same tool.

## Pulling new template content into an existing project

`agentsync refresh` walks the shipped templates (rules, skills, commands, agents) and compares each file against your `.ai/src/`. Use it after upgrading the CLI to inherit newly added rules/skills without re-running `init`.

It uses **three-way diff** via `.ai/.template-manifest` (written by `init` and updated on every refresh): the manifest records the template hash at scaffold/last-refresh time, so refresh can tell whether you've touched a file since the last sync.

- **Auto-update (silent)**: file you haven't touched + template moved → applied without a prompt.
- **NEW (prompt)**: file present in templates, never seen locally — offered for adding. Default Enter is **skip**; skipping records a manifest entry so the file isn't offered again (use `--include-deleted` to revisit).
- **CONFLICT (prompt with diff)**: both your version and the template diverged from the recorded baseline. Default Enter is **skip**; your local edits are not overwritten unless you type `[u]pdate`.
- **DELETED (silent)**: you removed a file locally that was once scaffolded → treated as an intentional decline. Pass `--include-deleted` to revisit.
- **USER_EDITED_NO_CHANGE (silent)**: you edited locally, template hasn't moved → not a conflict, your version stays.
- **Custom files (silent)**: anything in your `.ai/src/` that isn't in the templates is your own and is left alone.

Persistent overrides in `.ai/agent_sync.yaml` silence specific templates forever:

```yaml
template_overrides:
  declined:        # always-skip; never offered
    - rules/some-rule.md
  pinned:          # ignore template updates; keep your version even when it diverges
    - rules/my-version.md
```

Other behaviors:

- Scope by default = only categories that already have a subdirectory in your `.ai/src/`; pass `--only rules,skills,commands,agents` to opt into a category you don't have yet.
- AGENTS.md is excluded by default (almost always heavily customized); `--include-agents-md` surfaces it.
- `--dry-run` prints the plan without writing. `--yes` applies auto-updates and adds new files non-interactively; conflicts are always skipped under `--yes` (CI-friendly).
- Tool configs (`settings/`, `mcp/`, `hooks/`, `tools/`) are intentionally excluded — they're handled by `customize` / `simplify` / `resolve`.
- Commit `.ai/.template-manifest` to git so the team shares the same baseline; otherwise different developers will see different conflict sets.

`agentsync refresh --status` prints the current declined breakdown without prompting — useful when many overrides have accumulated. The list is split into **Persistent** (entries in `template_overrides.declined`) and **Local** (entries in `.template-manifest` whose file is missing on disk).

## Workspaces and shared content

A parent project at `workspace/.ai/src/` with sub-projects below — each with its own `.git` and `.ai/src/` — is supported. Two patterns to manage content shared between layers; pick whichever fits the use case better. They compose, but typically you'll pick one per category.

**Declarative inheritance — `shared:` in `agent_sync.yaml`.** The child names a parent path plus the categories it inherits:

```yaml
shared:
  path: "../"
  inherit: rules,skills,commands,agents
```

At sync time, AgentSync builds a transient shadow `.ai/src/` (child files first, then parent fillers; child wins on path collisions) and reads its sources from there. Sync then walks the shadow tree, so every enabled tool — including ones without parent-loading semantics (Codex, Cursor, Junie, Cline, Amazon Q) — receives the inherited content materialised into its own output. The shadow tree is built in memory and never touches disk. **Inherited files do not enter the child's `.template-manifest`** — refresh continues to consider only the child's own files; the parent owns its content.

A child skill with its own `SKILL.md` replaces the parent skill of that name, wherever each sits. A child directory named after a parent skill with no `SKILL.md` of its own extends it instead: its files join the parent's, a file at the same path replaces the parent's, and its `SKILL.append.md` is appended to the parent's `SKILL.md`.

**Interactive cleanup — `agentsync dedupe`.** When the child has copy-paste duplicates of parent files in its own `.ai/src/`, dedupe surfaces them by hash:

- Identical hash → `[d]elete / [k]eep / [v]iew` prompt. Deletion writes a `template_overrides.declined` entry when the file is a shipped template (so refresh won't re-offer it).
- Different hash → diff shown, decision left to the human; dedupe never auto-resolves a divergence.

Modes: default walks up to the nearest parent `.ai/src/` (bounded by the git repository boundary so it never escapes the current repo); `--against PATH` accepts an explicit `.ai/src/` or project root; `--workspace` runs across every nested `.ai/` below cwd in bottom-up alphabetical order.

**Detection — `agentsync doctor`.** Doctor's "Cross-project" section flags identical-hash duplicates as advisories and divergent files as info. Rules and skills with `category: governance` in their frontmatter are upgraded to advisories when divergent, with explicit "likely a mistake, not an override" framing. All cross-project findings are exit-code-0 advisories — visible during interactive runs, invisible to CI. Combine with `agentsync sync --workspace` for batch syncs across the tree.

**`category:` frontmatter.** Optional field on any rule/skill/command/agent. Today only `governance` carries behavior; `domain`, `workspace`, `project` are recorded but currently informational. Add it to a file when divergence between parent and child would be a mistake, not a deliberate override.

## Profiles (config-home variants)

A profile produces a second, self-contained config-home directory for a tool — e.g. a work `~/.claude-hub/` next to your personal `~/.claude/` (run it with `CLAUDE_CONFIG_DIR=~/.claude-hub`). Its content is the base `.ai/src/` overlaid with profile-only extras, so shared rules stay shared while work-specific rules/MCP live only in the profile. This is the right tool when you sync from `$HOME` and juggle multiple accounts/subscriptions per tool.

**Create one — `agentsync profile add <name>`.** Scaffolds, per enabled tool (or `--tools a,b`):

- a thin variant tool `.ai/src/tools/<tool>-<name>.yaml` — `base: <tool>` (inherits every unset field), `profile_home: ".<tool>-<name>"`, and config-home `targets.*.dest` (everything inside the home dir);
- an overlay dir `.ai/profiles/<name>/src/` — drop profile-only `rules/`, `skills/`, `commands/`, `agents/`, or `AGENTS.md` here (profile wins on path collisions with base);
- a `profiles:` entry in `agent_sync.yaml`:

  ```yaml
  profiles:
    hub:
      overlay: ".ai/profiles/hub"
      active: true
      tools: [claude-hub, codex-hub]
  ```

Pass `--adopt` to pull the existing contents of `~/.<tool>-<name>/` into the overlay first (rules/skills/commands/agents into the overlay, `.mcp.json`/`settings.json`/`hooks.json` into `.ai/src/tools/<variant>/`), so the first sync doesn't clobber a hand-built directory. Profile-specific MCP/settings/hooks live in that per-variant payload dir; everything else flows through the overlay.

**Sync.** `agentsync sync` syncs personal tools plus every `active: true` profile; `agentsync sync --profile <name>` syncs personal tools plus just that one. Each profile's tools render with a per-profile overlay layered over the base (and over an active `shared:` overlay, if any — the two compose). Profile outputs are gitignored and drift-protected like any other output.

**Inspect / remove.** `agentsync profile list` shows profiles, their tools, and config homes. `agentsync profile remove <name>` deletes the config-home output and variant files, then drops the `profiles:` entry (overlay sources under `.ai/profiles/<name>/` are kept).
