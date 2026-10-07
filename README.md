<div align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://raw.githubusercontent.com/yelmuratoff/exuno/main/assets/exuno.svg">
    <img src="https://raw.githubusercontent.com/yelmuratoff/exuno/main/assets/exuno_light.svg" alt="Exuno" width="400">
  </picture>

  <h3>One source → 15 AI tools. Stop copy-pasting rules.</h3>

  <p>
    <a href="https://github.com/yelmuratoff/exuno">
      <img src="https://img.shields.io/badge/built_with-rust-4EAA25?style=for-the-badge&logo=rust&logoColor=white" alt="Built with Rust">
    </a>
    <a href="https://www.gnu.org/licenses/gpl-3.0.html">
      <img src="https://img.shields.io/badge/license-GPL--3.0--only-4EAA25?style=for-the-badge" alt="GPL-3.0-only License">
    </a>
    <a href="https://github.com/yelmuratoff/exuno">
      <img src="https://img.shields.io/github/stars/yelmuratoff/exuno?style=for-the-badge&logo=github&color=4EAA25" alt="GitHub stars">
    </a>
  </p>
</div>

## The problem

Every AI coding tool wants instructions in its own format and directory: `.claude/CLAUDE.md`, `.cursor/rules/*.mdc`, `.github/instructions/*.instructions.md`, `AGENTS.md`, `.devin/rules/`...

Use more than one tool — or work on a team where different people use different tools? You end up maintaining the same rules in 5+ formats. They drift. They go stale. You copy-paste forever.

## The solution

Exuno syncs from a single source (`.ai/src/`) into **15 AI tools**: Claude Code, GitHub Copilot, Cursor, Gemini CLI, OpenAI Codex, Kimi Code, MiniMax Code, OpenCode, Devin Desktop (Windsurf), JetBrains Junie, Cline, Kiro, Amazon Q, Zed, Google Antigravity.

Write once → `exuno sync` → every tool gets instructions in its native format.

```
.ai/src/rules/testing.md
    ↓ exuno sync
├── .claude/rules/testing.md              # + @rules/testing.md import in CLAUDE.md
├── .cursor/rules/testing.mdc             # + globs/alwaysApply frontmatter
├── .github/instructions/testing.instructions.md  # + applyTo frontmatter
├── .devin/rules/testing.md               # + trigger: always_on frontmatter
├── .amazonq/rules/testing.md
├── AGENTS.md                             # inlined rule reference (Codex, Gemini, Junie, Kimi, MiniMax, OpenCode)
└── .rules                                # merged into single file (Zed)
```

The frontmatter above is the always-on default. Give a rule `paths:` frontmatter (a list of globs) and Exuno emits each tool's **scoped** trigger instead (`alwaysApply: false`, `applyTo: <globs>`, `trigger: glob`; Claude keeps `paths:`), so domain rules load only when matching files are touched — keeping the always-on context lean.

## Speed

Exuno was a Bash program until 0.37.0 and is a single Rust binary from
0.38.0. On the repository's own benchmark fixture — 389 source files, 13 tools
enabled, 3465 generated files — the same commands on the same machine:

| Command | Bash 0.37.0 | Rust 0.38.0 |
| --- | --- | --- |
| `exuno list` | 0.57 s | under 0.01 s |
| `exuno check` | 73.6 s | 0.25 s |
| `exuno sync` | 67.5 s | 2.5 s |
| `exuno sync --if-stale` | 0.18 s | 0.01 s |

`check` is the one that changes what you can do with it: at 74 seconds it could
not live in a pre-commit hook, and at a quarter of a second you stop noticing
it. The old engine spawned a process for every value it read; the new one reads
them in memory. Startup alone went from 38 ms to 5 ms, and the tool no longer
needs `bash` on the machine at all.

Method, the full table, and the ledger of everything else that changed are in
[`docs/perf/2026-09-19-rust-result.md`](docs/perf/2026-09-19-rust-result.md),
measured against [the Bash baseline](docs/perf/2026-09-13-bash-baseline.md)
recorded before the migration started.

## Why not just...?

- **...symlink the files?** Tools demand different extensions (`.mdc`, `.instructions.md`), different frontmatter, different nesting. Symlinks can't transform content — Exuno does.
- **...a shell script per tool?** You'd be writing the same copy / rename / header-injection logic 14 times. Exuno is that script, declarative (YAML), already tested on macOS, Linux, and Windows.
- **...stick to the one tool I use today?** Teammates pick different ones. Your future self might too. A single source file future-proofs you.
- **Zero runtime dependencies.** A single static binary. No Node, Python, `yq`, or `jq`. Install with one `curl | bash`.

<details>
<summary><strong>Table of contents</strong></summary>

- [The problem](#the-problem)
- [The solution](#the-solution)
- [Speed](#speed)
- [Why not just...?](#why-not-just)
- [Installation](#installation)
  - [Coming from AgentSync](#coming-from-agentsync)
- [Team Setup](#team-setup)
- [Quick Start](#quick-start)
- [Project Structure](#project-structure)
- [What Each Part Does](#what-each-part-does)
- [CLI Commands](#cli-commands)
  - [Sync options](#sync-options)
  - [Generate](#generate)
- [Tool Configuration](#tool-configuration)
- [Tool YAML Schema](#tool-yaml-schema)
  - [Key Fields](#key-fields)
- [Supported Tools](#supported-tools)
- [Format Conversions](#format-conversions)
- [Models and Providers Are Not Tools](#models-and-providers-are-not-tools)
- [Adding a New Tool](#adding-a-new-tool)
- [Automation](#automation)
- [Where generated files live](#where-generated-files-live)
- [How Sync Works](#how-sync-works)
- [Customization workflow](#customization-workflow)
- [How Resources Resolve](#how-resources-resolve)
- [Migrating from the 0.10 flat layout](#migrating-from-the-010-flat-layout)
- [Migrating an outdated Exuno project](#migrating-an-outdated-exuno-project)
- [Path Overrides](#path-overrides)
- [Migrating Existing Configurations](#migrating-existing-configurations)
  - [Step-by-step](#step-by-step)
  - [What gets overwritten](#what-gets-overwritten)
  - [Drift detection](#drift-detection)
  - [`exuno adopt` — promote an IDE edit back into source](#exuno-adopt--promote-an-ide-edit-back-into-source)
  - [Disabling sync for tools or categories](#disabling-sync-for-tools-or-categories)
  - [Letting a tool read `AGENTS.md` instead of its own file](#letting-a-tool-read-agentsmd-instead-of-its-own-file)
- [Workspaces — nested Exuno projects](#workspaces--nested-exuno-projects)
- [Profiles — multiple config homes per tool](#profiles--multiple-config-homes-per-tool)
- [Development](#development)
- [License](#license)
- [Uninstall](#uninstall)

</details>

## Installation

Requirements: `curl` and `tar`. Exuno is one static binary for **macOS** (Apple silicon and Intel), **Linux** (x86_64 and arm64) and **Windows** (x86_64).

```bash
curl -fsSL https://raw.githubusercontent.com/yelmuratoff/exuno/main/install.sh | bash
```

On Windows, from PowerShell:

```powershell
irm https://github.com/yelmuratoff/exuno/releases/latest/download/exuno-installer.ps1 | iex
```

This installer adds `exuno` only, without the `agentsync` link `install.sh` creates. In a project whose git hooks predate the rename, run `exuno setup-hooks` once; `exuno doctor` lists any hook still calling `agentsync`.

To install the exact release a project pins in `exuno_version` — what CI should do when outputs are committed — set `EXUNO_VERSION`; the same variable moves an existing install, and `exuno update <version>` does it from the CLI:

```bash
EXUNO_VERSION=0.45.0 curl -fsSL https://raw.githubusercontent.com/yelmuratoff/exuno/main/install.sh | bash
```

What the installer does:

1. Downloads the release archive for your platform from GitHub Releases and verifies its sha256
2. Places the binary at `~/.exuno/bin/exuno`
3. Creates the symlinks `exuno` and `agentsync` in `/usr/local/bin/` (falls back to `~/.local/bin/`) — the second keeps hooks and CI written before the rename working until 1.0

`exuno update` replaces the binary with the latest release, and `exuno update <version>` pins one. A release before 0.45.0 ships as `agentsync-<target>`, which both the installer and `update` still fetch. Releases before the first binary release have no archive: pinning to one installs from source (a git clone in `~/.exuno/` with `AGENTSYNC_HOME` in your shell config, as the installer always did), and such an install moves to the binary by itself the next time `exuno update` reaches a release that ships one.

### Coming from AgentSync

Exuno was called AgentSync up to 0.44. Everything keeps working until 1.0: the `agentsync` command, `AGENTSYNC_*` variables, `.ai/agent_sync.yaml`, `agentsync_version`, `metadata.agentsync-*` skill keys, and git hooks or CI gates that call `agentsync`. To move a project to the new names:

```bash
exuno migrate --legacy        # dry-run: lists every file, key, and skill still named agentsync
exuno migrate --apply --yes   # renames them and records format r3
exuno setup-hooks             # rewrites git hooks that still run the agentsync block
```

`exuno doctor` names whatever is left. A previous install in `~/.agentsync` is no longer used once you rerun the installer; it tells you how to remove it.

## Team Setup

One person connects the project; nobody else runs anything.

```bash
cd your-project
exuno init --tools claude,cursor   # wizard in a TTY; adopts existing config, then syncs
git add -A && git commit -m "chore: add exuno"
```

`init` keeps the tool config the project already has, generates the outputs for every enabled tool, and — with a `.github/` directory — offers a CI job that runs `exuno check`. After the commit:

- **Everyone else** runs nothing. `git pull` brings `CLAUDE.md`, `.claude/rules/`, `.cursor/rules/` and the rest, already current.
- **Anyone editing the rules** edits `.ai/src/`, runs `exuno sync`, and commits source and outputs together. `exuno setup-hooks` installs a pre-commit hook that enforces that, and `exuno check` in CI catches it when they skip the hook.
- **Everyone's agent** is told to edit `.ai/src/`: the shipped rules say so, and Claude Code gets a generated `PreToolUse` hook that blocks a write to a generated file and names the source instead.

Pin the engine so every machine and CI generate the same bytes: `exuno_version` in `.ai/exuno.yaml` is written by `init`, and `sync` and `check` stop when the running version differs. Install an exact release with `EXUNO_VERSION=<version>` on the installer or `exuno update <version>`.

Teams that would rather not commit generated files can pass `--outputs local`; then every clone needs `exuno` and `exuno setup-hooks`. See [Where generated files live](#where-generated-files-live).

## Quick Start

```bash
cd your-project
exuno init                        # 1. Interactive wizard in a TTY; auto-detect elsewhere
exuno enable claude cursor        # 2. Turn on the tools you use (prints where to edit)
exuno add mcp github --command …  # 3. (Optional) wire up shared MCP servers
exuno generate | pbcopy           # 4. (Optional) AI-generate a project-specific config
exuno sync                        # 5. Re-distribute after any change to .ai/src/
```

**What each step does:**

1. **`exuno init`** — Scaffolds the `.ai/` directory, adopts the tool config the project already has, and runs the first sync. In a terminal it opens a short wizard to pick tools, content sections, whether to commit generated files, and whether to add the CI gate; in scripts/CI it runs silently using auto-detection (`.claude/`, `.cursor/`, `CLAUDE.md`, ...) and those defaults. Only the payloads you opt into get scaffolded — other tools use shipped base templates at sync time. Useful flags: `--tools claude,cursor` (explicit list), `--content agents,rules` (narrow content), `--outputs local` (gitignore the outputs instead), `--existing replace` (regenerate over the project's own config instead of adopting it), `--ci github` (write the check workflow), `--no-sync` (skip the first sync), `--no-templates` (empty `.ai/src/` layout without shipped starters), `--no-detect` (skip tool auto-detection), `--yes` (accept defaults), `--dry-run` (preview). Safe to run twice — if `.ai/src/` already exists, it skips.

2. **`exuno enable <tool>`** — Adds the tool to `tools.enabled` _and_ scaffolds editable copies of its settings / hooks at `.ai/src/tools/<tool>/`, then prints the exact file path to edit plus the shared MCP path. Pass `--no-scaffold` to skip materializing files; pass `--yes` to accept the TTY confirm non-interactively.

3. **`exuno add mcp <server>`** — Writes an MCP server entry into the shared `.ai/src/mcp.json`. On the next `sync`, every enabled MCP target gets the server map in its native format. Put a divergent map at `.ai/src/tools/<tool>/mcp.json`; `exuno customize <tool> mcp` scaffolds it when that target ships a copyable base.

4. **`exuno generate`** — Prints a detailed prompt that you paste into any AI (Claude, ChatGPT, Gemini). The AI analyzes your project description and generates a complete `.ai/src/` config tailored to your stack: project-specific AGENTS.md, rules, skills, commands, agents, and settings. Pass optional context: `exuno generate "React + Next.js + Prisma"`. Use `| pbcopy` (macOS) or `| xclip` (Linux) to copy to clipboard.

5. **`exuno sync`** — Reads each enabled tool's config (user override + shipped base — see [How Resources Resolve](#how-resources-resolve)), then copies and transforms your source files into tool-specific formats. Rules get renamed (`.mdc` for Cursor, `.instructions.md` for Copilot), frontmatter headers are added, commands are converted to TOML for Gemini, agents get the right extensions, and settings/MCP/hooks are placed where each tool expects them. Also manages the `.gitignore` block that matches your `outputs:` mode — see [Where generated files live](#where-generated-files-live).

After `sync`, tool-specific directories appear (`.claude/`, `.cursor/`, `.github/`, `.devin/`, etc.), each with instructions in that tool's expected format.

> **Important:** `exuno sync` **overwrites** generated tool directories entirely. `exuno init` adopts the config a project already has, and `exuno adopt <file>` promotes a single file at any time — but a sync you run against untouched tool directories replaces them from `.ai/src/`. See [Migrating Existing Configurations](#migrating-existing-configurations).

## Project Structure

Exuno supports two source layouts:

**Structured (default, created by `init`):**

```
.ai/
├── exuno.yaml             # project config (tools.enabled, version pin, paths)
└── src/                        # Source of truth. Edit ONLY here.
    ├── AGENTS.md               # Agent identity: role, approach, principles
    ├── rules/                  # Rules — always-on, or paths:-scoped (load on-demand)
    │   ├── core.md
    │   └── git.md
    ├── skills/                 # (optional) on-demand step-by-step recipes
    │   └── .../SKILL.md
    ├── commands/               # (optional) custom slash commands
    ├── agents/                 # (optional) subagent personas
    ├── mcp.json                # (optional) shared MCP servers for compatible targets
    └── tools/                  # (optional) per-tool overrides
        ├── claude.yaml         #   tool YAML (same file as before 0.11)
        └── claude/             #   per-tool payload dir (NEW in 0.11)
            ├── settings.json   #     settings override
            ├── hooks.json      #     hooks override
            └── mcp.json        #     per-tool MCP override (shadows mcp.json above)
```

> **Note:** `init` is minimal. `mcp.json` and every file under `tools/<tool>/` are _overrides_ — they appear only when you opt in via `exuno enable`, `exuno customize`, or `exuno add mcp`. Missing overrides fall back to shipped base templates automatically — see [How Resources Resolve](#how-resources-resolve) and [Customization workflow](#customization-workflow).

**Flat (auto-detected):** `.ai/AGENTS.md`, `.ai/rules/`, `.ai/skills/`, `.ai/tools/`

## What Each Part Does

| Source        | Purpose                                                                                                                                                                                                                                                                                                                                       | Tools that use it                                           |
| ------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------- |
| **AGENTS.md** | Agent identity — role, approach, principles. Copied as-is (renamed per tool: `CLAUDE.md`, `GEMINI.md`, `.junie/AGENTS.md`, `00-context.md`).                                                                                                                                                                                                                      | All                                                         |
| **rules/**    | Always-on constraints by default. Add `paths:` frontmatter (a list of globs) to scope a domain rule so it loads only when matching files are touched — translated to each tool's native trigger (Claude keeps `paths:`, Cursor `globs`+`alwaysApply:false`, Copilot `applyTo`, Windsurf/Antigravity `trigger: glob`). One file per topic; keep the always-on set lean so individual rules aren't diluted. | All                                                         |
| **skills/**   | On-demand recipes in the open [agentskills.io](https://agentskills.io) format. Each skill = directory with `SKILL.md` + optional `references/`, `scripts/`, `assets/`. Description is the trigger (keyword-first, pushy about phrasings, narrow about the domain, ≤1024 chars). `Gotchas` section prevents repeated mistakes. Inlined as index for tools without native skills support. | All except MiniMax Code; its project skill path is unverified |
| **commands/** | Custom slash commands. `review.md` → `/project:review`. Support `$ARGUMENTS` and `` !`shell` `` syntax. Auto-converted to TOML for Gemini. For tools without a native commands surface, Exuno converts commands to generated skills or an inlined index. | Claude, Cursor, Copilot (`.prompt.md`), Gemini (TOML), Junie, Cline, Windsurf, Antigravity, OpenCode; Codex and Kimi Code (as skills); Amazon Q, Zed (inlined) |
| **agents/**   | Subagent personas. Isolated context, restricted tools. Frontmatter: `model`, `tools`, `readonly`. Converted when the target needs a different schema. | Claude, Cursor, Copilot (`.agent.md`), Gemini, Junie, Codex (TOML), Amazon Q (JSON), OpenCode (safe MD) |
| **settings/** | Permissions & config. Per-tool files (`claude.json`, `gemini.json`, `codex.toml`, `opencode.json`, `zed.json`). Controls allow/deny rules. Claude hooks also go here. | Claude, Gemini, Codex, OpenCode, Zed |
| **mcp.json**  | Shared canonical `mcpServers` map. Copied to compatible targets and converted into OpenCode's top-level `mcp` map. | Claude, Cursor, Windsurf, Junie, Amazon Q, Kimi Code, MiniMax Code, OpenCode |
| **hooks/**    | Event hooks and native project plugins. Per-tool overrides can be JSON or TypeScript. | Cursor, Copilot, Codex, Windsurf, OpenCode |
| **tools/**    | YAML configs — define where and how files are synced per tool.                                                                                                                                                                                                                                                                                | —                                                           |

## CLI Commands

```
exuno <command> [options]
```

| Command                  | Alias | Description                                                                                    |
| ------------------------ | ----- | ---------------------------------------------------------------------------------------------- |
| `init [dir]`             |       | Create `.ai/` structure with starter templates                                                 |
| `sync`                   |       | Sync to all enabled tools (`--only`, `--skip`, `--profile`, `--dry-run`, `--force`, `--if-stale`, `--quiet`, `--json`, `--workspace`) |
| `rollback [backup-id]`   |       | Restore managed targets from the latest or selected backup (`--list`, `--dry-run`, `--force`, `--yes`) |
| `check`                  |       | Verify outputs match source (CI-friendly, exit code 0/1)                                       |
| `enable <tools…>`        |       | Opt in to one or more tools (scaffolds editable settings/hooks payloads)                        |
| `disable <tools…>`       |       | Opt out of one or more tools                                                                    |
| `add <kind> <name>`      |       | Scaffold a `rule`, `skill`, `command`, `subagent`, or `mcp` server                              |
| `customize <tool> [res]` |       | Create a per-field override for a tool                                                          |
| `simplify [tool]`        |       | Remove override fields that match the base (`--apply`)                                          |
| `show <tool>`            |       | Show effective (merged) config for a tool                                                       |
| `diff [tool]`            |       | Show user overrides vs base defaults                                                            |
| `resolve`                |       | Interactively reconcile overrides with base values                                              |
| `refresh`                |       | Pull new template files into existing `.ai/src/` (three-way diff; `--status` to list declined) |
| `dedupe`                 |       | Interactively remove source files duplicated against a parent `.ai/src/`                       |
| `migrate`                |       | Print and copy an AI prompt that safely upgrades an existing Exuno configuration            |
| `adopt <dest>`           |       | Promote a manual edit in a generated file back into `.ai/src/` (`--all` for every drifted file) |
| `profile <cmd>`          |       | Manage config-home profiles: `add`, `list`, `remove` (work/personal tool variants)             |
| `doctor`                 |       | Validate setup and surface drift / config warnings / cross-project advisories                  |
| `generate [context]`     | `gen` | Print AI prompt for project-specific config generation                                         |
| `setup-hooks`            |       | Install the git hooks that suit the project's `outputs:` mode (`--pre-commit` in `local` mode)  |
| `shell-init [zsh\|bash]` |       | Print a shell hook that auto-syncs the nearest project on directory change                      |
| `export`                 |       | Bundle `.ai/src/` into a shareable archive                                                      |
| `import <src>`           |       | Import config from a GitHub repo, archive, or directory                                         |
| `list`                   | `ls`  | Show configured tools and status                                                               |
| `skills list\|show\|check` |     | Inspect effective project skills and check their `SKILL.md` metadata (`--profile <name>`)     |
| `mcp list\|show\|validate\|render\|use` |     | Inspect a selected offline MCP catalog or prepare a per-tool source without running servers     |
| `update`                 |       | Replace the binary with the latest release, or `update <version>` to pin a release tag         |
| `upgrade-config`         |       | Re-pin `exuno_version` in `exuno.yaml`                                                 |
| `release`                |       | Bump version, tag, and push (maintainer)                                                        |
| `version`                | `-v`  | Print version                                                                                  |
| `help`                   | `-h`  | Show help                                                                                      |

### Skill categories

A directory under `skills/` without its own `SKILL.md` is a category, so `.ai/src/skills/flutter/bloc/SKILL.md` and `.ai/src/skills/backend/auth/SKILL.md` can sit side by side, up to four category levels deep. Tools disagree on nested skills (Claude Code, VS Code Copilot, and Gemini CLI skip them), so `sync` lands every skill flat at `<dest>/<name>/` for every tool, and the inlined index groups entries under a heading per category. Keep leaf names unique across categories: two skills sharing a name stop `sync`. `targets.skills.include`/`exclude` match a skill's name or its category path, so `exclude: cloudflare/*` drops a whole category for one tool. Scaffold into a category with `exuno add skill bloc --category flutter`; category names follow the same lowercase-kebab rule as skill names, and `skills check` and `doctor` flag a directory that breaks it. Moving a shipped skill such as `commit` into a category keeps it tracked: `refresh` finds it there by name and updates it in place.

### Inspecting skills

`exuno skills list` reads the effective `source.skills` tree, including shared and bundled skills, and names each skill's category. Use `--profile <name>` to inspect a profile, or `--include` and `--exclude` to filter by name or category path. `exuno skills show <name>` displays a skill's description, category, source path, and declared `license` and `compatibility` when present. `exuno skills check` reports missing or malformed required metadata, empty or too-deep categories, and names two skills share; of those, `sync` refuses only the shared name.

The [Agent Skills specification](https://agentskills.io/specification) defines `name`, `description`, `compatibility`, and an optional string-valued `metadata` map. For project-owned skills, use that map when a concise card needs extra human context:

```yaml
metadata:
  exuno-use-when: Review a selected diff before merging
  exuno-not-for: Writing or fixing the code under review
  exuno-requirements: A selected diff and access to the repository
```

`skills show` labels these entries as **unverified annotations**; it does not probe tools, authorize actions, or select a skill automatically. The built-in check covers the required fields and common scalar forms. Use the [reference validator](https://agentskills.io/specification#validation) (`skills-ref validate <skill-dir>`) for full format validation. [OASF](https://github.com/agntcy/oasf) provides a separate capability taxonomy; Exuno does not infer OASF mappings from a skill description.

For skills outside this project's effective source tree, `exuno skills catalog list/show --catalog FILE` reads an explicitly selected, manually curated catalog. Optional `--source ALIAS=LOCAL_REPO` inspects metadata from a full pinned commit in a local Git repository. It does not fetch, install, execute, or verify the curator's suitability claims. See the [experimental catalog contract](docs/skill-cards.md) and [pinned example](docs/examples/skill-cards/pilot/catalog.tsv).

### Inspecting an MCP catalog

`exuno mcp list/show/validate/render --library DIR` reads JSON manifests from a selected local catalog. `list` prints IDs and titles, `show` prints the selected manifest's original bytes, `validate` checks one entry or the whole catalog, and `render <id>[@variant]` prints the selected connection as Exuno MCP source JSON. `mcp use <id>[@variant] --tool SLUG` previews a per-tool source; `--apply` writes it, and `--merge --apply` extends an existing per-tool JSON source with one server. Replacing an ID requires `--replace <id>`. These commands do not start a server or probe an endpoint, and `use` does not run `sync` for you. See the [MCP catalog contract](docs/mcp-library.md) for the bounded manifest format and optional `library.mcp.path` setting. The opt-in [pilot catalog](catalog/mcp/README.md) includes Microsoft Learn, Context7, and Octocode with their requirements and source references.

### Sync options

```bash
exuno sync                        # All enabled tools
exuno sync --only claude,cursor   # Only specified tools
exuno sync --skip gemini          # All except specified
exuno sync --profile hub          # Personal tools + the named profile (config-home variant)
exuno sync --dry-run              # Preview without writing
exuno sync --force                # Overwrite edited files and prune hand-added files in generated dirs
exuno sync --workspace            # Run sync in every .ai/ below cwd (bottom-up alphabetical)
exuno sync --quiet                # Only warnings, errors, and the closing [DONE] line
exuno sync --json                 # One JSON summary object on stdout after a successful run
exuno rollback --list             # List init/sync/rollback snapshots
exuno rollback --dry-run          # Preview restoring the latest snapshot
exuno rollback <backup-id> --yes  # Restore one snapshot non-interactively
```

The sync log is written to stderr, so `exuno sync > out.json` captures nothing but the `--json` summary and `2>&1` merges the log back in. The JSON object is the stable contract for scripts — `dry_run`, `synced`, `total`, `skipped`, `written`, `preserved`, `backup` — and its fields are only ever added to; the human log may change between releases.

### Generate

```bash
exuno generate                    # Generate bootstrap prompt
exuno generate | pbcopy           # Copy to clipboard (macOS)
exuno generate "React + Next.js"  # With project context
exuno migrate                     # Print and auto-copy a safe upgrade prompt
```

Works like `claude /init` — generates a prompt that you paste into any AI (Claude, ChatGPT, Gemini). The AI analyzes your codebase description and creates a complete `.ai/src/` config: AGENTS.md, rules, skills, commands, and agents tailored to your project's stack and conventions.

## Tool Configuration

Each tool = one YAML file in `.ai/src/tools/`. Filename = tool identifier for `--only`/`--skip`. Files starting with `_` are ignored.

Example — `.ai/src/tools/claude.yaml`:

```yaml
name: "Claude Code"
enabled: true

targets:
  agents:
    dest: "CLAUDE.md"
  rules:
    dest: ".claude/rules"
  skills:
    dest: ".claude/skills"
  commands:
    dest: ".claude/commands"
  subagents:
    dest: ".claude/agents"
  settings:
    source: ".ai/src/settings/claude.json"
    dest: ".claude/settings.json"
  mcp:
    source: ".ai/src/mcp/claude.json"
    dest: ".mcp.json"
```

## Tool YAML Schema

```yaml
name: "Tool Name"
enabled: true

targets:
  agents:
    dest: ".tool/AGENTS.md"
    # source: ".ai/src/custom.md"

  rules:
    dest: ".tool/rules"
    # source: ".ai/src/my-rules"
    # extension: ".mdc"
    # header: "---\nkey: value\n---"
    # scoped_header: "---\nglobs: '{globs}'\nalwaysApply: false\n---"
    # include: "flutter-*.md"
    # exclude: "secret-*.md"
    # append_imports: true
    # merge_to_file: true
    # inline_into_agents: true

  skills:
    dest: ".tool/skills"
    # source: ".ai/src/my-skills"
    # include: "flutter*"
    # exclude: "python*"
    # inline_into_agents: true

  commands:
    dest: ".tool/commands"
    # extension: ".prompt.md"
    # format: "toml"

  subagents:
    dest: ".tool/agents"
    # extension: ".agent.md"
    # format: "toml"

  settings:
    source: ".ai/src/settings/tool.json"
    dest: ".tool/settings.json"

  mcp:
    source: ".ai/src/mcp/tool.json"
    dest: ".tool/.mcp.json"

  hooks:
    source: ".ai/src/hooks/tool.json"
    dest: ".tool/hooks.json"

  guard:
    dest: ".tool/hooks/agentsync-guard.sh"
    # profile_scoped: false

# post_sync: "npx prettier --write .tool/**/*.mdc"   # off unless EXUNO_ALLOW_POST_SYNC=true
```

`post_sync` runs arbitrary shell, so the engine skips it with a warning unless
`EXUNO_ALLOW_POST_SYNC=true` is set in the environment — the in-repo
`exuno.yaml` cannot grant that, so cloning and syncing an untrusted
repository never runs its hook. `EXUNO_SKIP_POST_SYNC=true` or
`post_sync.skip: true` in `exuno.yaml` disables it again, and `check`
always skips it.

Quote any value that holds a `#`. Exuno reads an unquoted value up to the
first `#`, with or without a space before it, so `url: https://x.dev/#top`
reads as `https://x.dev/`; `url: "https://x.dev/#top"` keeps the whole URL.
The same holds in `exuno.yaml`.

### Key Fields

| Field                         | Purpose                                                                                                                      |
| ----------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| `extension`                     | Rename file extension (`.mdc`, `.instructions.md`, `.agent.md`, `.prompt.md`)                                          |
| `header`                        | Prepend text to each file (YAML frontmatter for Cursor, Windsurf, Copilot)                                             |
| `scoped_header` (rules)         | Header used instead of `header` for a rule with `paths:` frontmatter; `{globs}` becomes its comma-joined globs, `{globs_list}` a quoted YAML list (Cursor, Copilot, Windsurf, Antigravity, Kiro) |
| `append_imports`                | Append `@rules/*` import lines to AGENTS file (Claude)                                                                 |
| `merge_to_file`                 | Merge all rules into a single file (Zed)                                                                               |
| `inline_into_agents` (rules)    | Append lightweight rule REFERENCES (name + title) into AGENTS file (Codex, Gemini, Junie, Kimi Code, MiniMax Code, OpenCode) |
| `inline_into_agents` (skills)   | Append lightweight skill INDEX (name + description) into AGENTS file (Amazon Q, Zed)                                    |
| `as_skills` (commands)          | Emit each command as a generated skill at `<skills.dest>/command-<name>/SKILL.md` (Codex, Kimi Code)                  |
| `inline_into_agents` (commands) | Append a `## Commands` index (`` `/<name>` — description ``) into AGENTS file (Amazon Q, Zed)                          |
| `prepend_agents`                | Prepend AGENTS.md content before merged rules (Zed)                                                                    |
| `format: "toml"`                | Auto-convert MD→TOML (Gemini commands, Codex subagents)                                                                |
| `format: "amazonq_json"`        | Auto-convert subagent MD→Amazon Q CLI custom-agent JSON (Amazon Q subagents)                                           |
| `format: "opencode_md"`         | Convert portable subagent frontmatter to safe OpenCode Markdown (OpenCode subagents)                                  |
| `format: "kiro_md"`             | Convert portable subagent frontmatter to a Kiro Markdown agent, tools mapped to Kiro's tags (Kiro subagents)           |
| `format: "opencode_json"`       | Compose canonical `mcpServers` into OpenCode's top-level `mcp` settings map                                            |
| `source` (settings/mcp/hooks)   | Optional declared source; canonical `.ai/src/tools/<tool>/<resource>.<ext>` overrides it automatically                 |
| `legacy_dest`                   | Where an earlier config wrote the target; sync removes the files there the manifest records once it moves (Windsurf)   |
| `guard` (target)                | Copy the tool's write-guard script to `dest` and mark it executable; override the base at `.ai/src/tools/<tool>/guard.sh` and register it from the tool's settings (Claude) |
| `profile_scoped: false`         | On any target: `exuno profile add` keeps the base `dest` instead of rewriting it into the profile's config home     |
| `profile_supported: false`      | Refuse config-home profiles for a client that reads only project-root files                                             |
| `adoptable: false` (agents)     | Refuse `adopt` when the generated agents file cannot safely round-trip into its source                                 |

[`lib/templates/tools/_TEMPLATE.yaml`](lib/templates/tools/_TEMPLATE.yaml)
documents every option, including the ones this table leaves out.

## Supported Tools

| Tool                   | Config             | Syncs                                                                                                      |
| ---------------------- | ------------------ | ---------------------------------------------------------------------------------------------------------- |
| **Claude Code**        | `claude.yaml`      | CLAUDE.md, rules, skills, commands, agents, settings.json, .mcp.json                                       |
| **GitHub Copilot**     | `copilot.yaml`     | copilot-instructions.md, .instructions.md rules, skills, .prompt.md commands, .agent.md agents, hooks.json |
| **Cursor**             | `cursor.yaml`      | AGENTS.md, .mdc rules, skills, commands, agents, mcp.json, hooks.json                                      |
| **Gemini CLI**         | `gemini.yaml`      | GEMINI.md (+inlined rules), skills, commands (MD→TOML), agents, settings.json                              |
| **OpenAI Codex**       | `codex.yaml`       | AGENTS.md (+inlined rules), skills, commands (as `command-*` skills), subagents (MD→TOML), hooks.json, config.toml |
| **Kimi Code**          | `kimi.yaml`        | .kimi-code/AGENTS.md (+inlined rules), skills, commands (as `command-*` skills), mcp.json                         |
| **MiniMax Code**       | `minimax.yaml`     | AGENTS.md (+inlined rule references), project .mcp.json                                                          |
| **OpenCode**           | `opencode.yaml`    | AGENTS.md (+inlined rules), skills, commands, subagents (safe MD), settings + converted MCP in opencode.json, agentsync.ts plugin |
| **Devin Desktop (Windsurf)** | `windsurf.yaml` | AGENTS.md and `.devin/` rules (trigger frontmatter), skills, workflows (commands), mcp_config.json, hooks.json; removes its old `.windsurf/` outputs |
| **JetBrains Junie**    | `junie.yaml`       | .junie/AGENTS.md (+inlined rules), skills, commands, agents, mcp.json                                      |
| **Cline**              | `cline.yaml`       | AGENTS.md, `.cline/` rules, skills, workflows (commands); removes its old `.clinerules/` outputs           |
| **Kiro**               | `kiro.yaml`        | AGENTS.md, `.kiro/steering/` (inclusion frontmatter), skills, commands as `command-*` skills, agents (MD→Kiro MD), settings/mcp.json |
| **Amazon Q**           | `amazonq.yaml`     | 00-context.md, .amazonq/rules/, +inlined skills index, +inlined commands index, mcp.json, cli-agents (MD→JSON) |
| **Zed**                | `zed.yaml`         | .rules (prepend AGENTS.md + merged rules), +inlined skills index, +inlined commands index, settings.json   |
| **Google Antigravity** | `antigravity.yaml` | GEMINI.md, .agents/rules (trigger frontmatter), .agents/skills, .agents/workflows (commands)               |

## Format Conversions

Exuno auto-converts between formats during sync:

| Source format  | Target format                                    | Used by                     |
| -------------- | ------------------------------------------------ | --------------------------- |
| Rules `.md`    | `.mdc` + YAML frontmatter                        | Cursor                      |
| Rules `.md`    | `.instructions.md` + `applyTo` header            | Copilot                     |
| Rules `.md`    | `.md` + `trigger: always_on` header              | Windsurf                    |
| Rules `.md`    | Single merged file                               | Zed                         |
| Rules `.md`    | Inline references (name + title) in AGENTS.md    | Codex, Gemini, Junie, Kimi Code, MiniMax Code, OpenCode |
| Skills dirs    | Inline index (name + description) in AGENTS.md   | Amazon Q, Zed               |
| AGENTS.md      | Copied as `00-context.md` in rules directory     | Amazon Q                    |
| AGENTS.md      | Prepended before merged rules                    | Zed                         |
| Commands `.md` | `.toml` (prompt field, `!{}` syntax, `{{args}}`) | Gemini CLI                  |
| Commands `.md` | `.prompt.md`                                     | Copilot                     |
| Agents `.md`   | `.agent.md`                                      | Copilot                     |
| Agents `.md`   | `.toml` (developer_instructions field)           | Codex                       |
| Agents `.md`   | `.json` (Amazon Q CLI custom-agent)              | Amazon Q                    |
| Agents `.md`   | Safe OpenCode Markdown (`mode` + permissions)    | OpenCode                    |
| MCP `mcpServers` | OpenCode top-level `mcp` (`local` / `remote`)  | OpenCode                    |

You write everything in Markdown. Exuno handles the rest.

## Models and Providers Are Not Tools

Exuno targets coding tools and their filesystem formats, not model vendors. Kimi and GLM models used through Claude Code, Cline, or OpenCode continue to use that tool's target; there is deliberately no `glm.yaml` or `zai.yaml`. MiniMax Code has a standalone CLI with its own project files; a MiniMax model used through another client still uses that client's target.

- **Kimi model in another tool:** configure the provider with [Kimi's official third-party-agent setup](https://www.kimi.com/code/docs/en/third-party-tools/other-coding-agents), then keep syncing the existing Claude/Cline/OpenCode target. Use the `kimi` target only for the standalone Kimi Code CLI.
- **GLM Coding Plan:** authenticate Z.AI using its official [Claude Code](https://docs.z.ai/devpack/tool/claude) or [OpenCode](https://docs.z.ai/devpack/tool/opencode) flow. Keep API keys in the provider's credential store or environment, never in `.ai/src/`.
- **OpenCode MCP:** shared `.ai/src/mcp.json` is converted into OpenCode's top-level `mcp` map. Use `.ai/src/tools/opencode/mcp.json` for a divergent canonical server map. Move any existing `mcp` field out of `.ai/src/tools/opencode/settings.json` before enabling canonical MCP; `sync` and `doctor` name both files when ownership is ambiguous.
- **OpenCode hooks:** customize `.ai/src/tools/opencode/hooks.ts`; Exuno owns only `.opencode/plugins/agentsync.ts` and preserves sibling plugins. Custom tools under `.opencode/tools/`, other plugins, themes, TUI preferences, and credentials remain tool-owned.
- **Kimi hooks and agents:** Kimi Code exposes built-in agents but no project custom-agent surface. Its hooks live globally in `$KIMI_CODE_HOME/config.toml`, so Exuno intentionally leaves that file untouched.

```bash
exuno enable kimi opencode     # enable the standalone coding tools
exuno sync

npx @z_ai/coding-helper           # configure GLM in an existing supported tool
opencode auth login               # choose Z.AI Coding Plan for OpenCode
```

## Adding a New Tool

A tool Exuno does not ship is a YAML file in your own project. Start from
the closest shipped tool rather than an empty file:

```bash
exuno customize cursor --full        # writes .ai/src/tools/cursor.yaml, the full base
cp .ai/src/tools/cursor.yaml .ai/src/tools/newtool.yaml
# Edit newtool.yaml: at least `name:` and one target's `dest:`
exuno enable newtool
exuno sync --only newtool
```

Every field is documented in
[`lib/templates/tools/_TEMPLATE.yaml`](lib/templates/tools/_TEMPLATE.yaml) in
this repository. A file whose name starts with `_` is never read as a tool, so
a copy you keep for reference costs nothing.

## Automation

Generated outputs regenerate from `.ai/src/`, so they go stale whenever you edit the source (or pull a teammate's change) and forget to re-sync. Three mechanisms keep them current without you remembering — pick whichever fits your workflow (they compose).

### Shell hook (most hands-off)

Add this to your `~/.zshrc` (or `~/.bashrc` with `bash`):

```bash
eval "$(exuno shell-init zsh)"
```

This regenerates the hook from `exuno` each session — the recommended form, since upgrades and fixes apply automatically without re-editing your rc (the same pattern as `direnv`, `starship`, and `zoxide`). The hook runs `exuno sync --if-stale` when the current directory itself is an `.ai/` project root (including when a shell opens there). It does not walk up to a parent project while you navigate its descendants. The command is a no-op — and silent — when nothing changed. Set `EXUNO_NO_AUTO_SYNC=1` to disable without removing the line.

Prefer to avoid the per-session `exuno` call? Freeze a copy instead with `exuno shell-init zsh >> ~/.zshrc`, but re-run it after each upgrade to pick up changes.

### Git hooks

```bash
exuno setup-hooks                 # the hooks this project's outputs: mode needs
exuno setup-hooks --pre-commit    # local mode: also add a pre-commit sync
```

`setup-hooks` installs what the project's `outputs:` mode calls for. With `committed` (the default) that is a single pre-commit gate: it runs `sync --if-stale` and **fails the commit** when a generated file would be left out of it, so outputs never lag source. With `local` it installs `post-merge` and `post-checkout` hooks that run `exuno sync` after `git pull` / `git checkout`, and `--pre-commit` adds a hook that runs `sync --if-stale` before each commit; those three are non-fatal — a failed sync warns but never blocks the git operation. `EXUNO_SKIP_HOOKS=1` turns any installed hook into a no-op. Safe to run multiple times: a current Exuno block is left alone, one an older release installed is rewritten in place, and the rest of the hook is kept.

### Manual / CI

```bash
exuno sync --if-stale    # sync only if source changed since the last sync
exuno check              # verify outputs match source (exit 0/1, CI gate)
```

`--if-stale` is the cheap probe the shell and pre-commit hooks build on; `check` is the authoritative drift gate for CI.

## Where generated files live

`outputs:` in `.ai/exuno.yaml` decides whether generated tool files are committed or regenerated on every machine. `exuno init` writes `committed`; pass `--outputs local` to choose the other mode.

| Mode                    | In git                                               | Who runs `exuno`                                 |
| ----------------------- | ---------------------------------------------------- | ---------------------------------------------------- |
| `committed` *(default)* | `.ai/src/`, generated outputs, `.ai/.sync-manifest`  | Whoever edits `.ai/src/` (`sync`), plus CI (`check`) |
| `local`                 | `.ai/src/` only                                      | Every clone, after every pull (`setup-hooks`)        |

In both modes `exuno sync` manages a block in `.gitignore` between `AI SYNC GENERATED START/END` markers: `local` lists every generated path and the manifest, `committed` lists only profile config homes, which are personal in either mode. The manifest always shares the git status of the outputs it describes — that is what keeps a teammate's `git pull` from looking like a manual edit. A project without an `outputs:` key behaves as `local`, or as `committed` when it already set `gitignore.update: false`.

### Engine-owned skills

The `exuno` skill documents Exuno itself, so it is versioned with the engine instead of being copied into every project where it would go stale. It ships inside the binary, built from `lib/templates/base-src/skills/`, and is resolved at sync time, after any `shared:` parent, so precedence reads project → shared parent → engine. Upgrade the engine and the next `sync` in any project emits the current version, with no prompt and no merge.

To add to it and keep the updates, create a directory named `exuno` with no `SKILL.md` of its own, at `.ai/src/skills/exuno/` or inside any category. Its files join the engine's, and a file at the same path as one of the engine's replaces it. A `SKILL.append.md` there is appended to the engine's `SKILL.md` at sync, so the agent learns about your additions; it never reaches a tool as a file of its own. The same works for a skill a `shared:` parent provides.

To diverge completely, keep your own `SKILL.md` in that directory — a project copy with its own `SKILL.md` always wins, and engine upgrades stop reaching it. To drop the layer entirely, set `base_skills: false` in `.ai/exuno.yaml`.

Projects scaffolded before this carry their own copy, which shadows the engine's. `exuno migrate` reports it and `exuno migrate --apply` removes the copy when it is unedited, leaving an edited one in place as the deliberate override it is.

### Project format revision

`format:` in `.ai/exuno.yaml` records which migrations a project has been walked through. It is a small counter bumped only when a project actually needs a step — unlike `exuno_version`, which moves on every patch — so the reminder appears exactly when something applies and never otherwise. `init` writes the current revision, `migrate --apply` records it, and nothing else touches it. A project behind the engine is flagged on the next interactive command and by `exuno doctor`.

### Keeping agents on the source

Generated files are output, so an agent that edits them loses the change on the next sync. Three layers prevent that: the shipped `AGENTS.md` and `rules/core.md` state where instructions live, Claude Code receives a generated `PreToolUse` hook (`.claude/hooks/agentsync-guard.sh`) that blocks a write to any path in `.ai/.sync-manifest` and names the source instead, and `sync` refuses to overwrite a generated file edited since the last run. Replace the hook per project at `.ai/src/tools/claude/guard.sh`, or remove the `hooks` block from your settings override to drop it.

`exuno_version` in `exuno.yaml` pins the engine. With committed outputs every machine and CI must generate byte-identical files, so `sync` and `check` stop when the running version differs from the pin: match it with `exuno update <version>` (or `EXUNO_VERSION=<version>` on the installer), or move the pin with `exuno upgrade-config` and commit the re-synced outputs. In `local` mode the mismatch is a warning by default. Set `version_pin.mode: strict` to make a local mismatch fatal as well; `warn` preserves the default. Unknown modes are rejected before a sync can write outputs.

```yaml
version_pin:
  mode: strict # or warn (default)

# Scalar shorthand:
version_pin: strict # or warn
```

## How Sync Works

1. Reads `config.yaml` for default source paths.
2. Auto-detects structured (`.ai/src/`) or flat (`.ai/`) layout.
3. For each tool YAML:
   - Copies AGENTS.md → tool-specific name (or as `00-context.md` for directory-based tools)
   - Syncs rules with extension/header/merge transforms
   - If `inline_into_agents` (rules): appends lightweight rule references (name + title) to agents file
   - If `prepend_agents` (rules): prepends AGENTS.md content before merged rules
   - Syncs skills directories (or inlines skill index into agents file if `inline_into_agents`)
   - Syncs commands. Four modes pick the first that fits: native `dest` → `format: toml` → `as_skills` (writes `<skills.dest>/command-*/SKILL.md`) → `inline_into_agents` (appends `## Commands` index to AGENTS file)
   - Syncs subagents (with optional extension rename or MD→TOML)
   - Resolves settings / MCP / hooks per the base + override rules below
   - Runs the optional `post_sync` command, but only when `EXUNO_ALLOW_POST_SYNC=true` comes from the environment rather than the repository
4. Updates `.gitignore`
5. Disabled tools get their generated files cleaned up automatically.

## Customization workflow

Three commands cover every customization, each with a single responsibility.

```
┌───────────────────────────────────────────────────────────────────────────────┐
│  exuno enable <tool>                                                      │
│    → adds tool to tools.enabled                                               │
│    → scaffolds .ai/src/tools/<tool>/{settings,hooks}.<ext> from base          │
│    → prints the exact file path to edit                                       │
│                                                                               │
│  exuno add mcp <server> [--command|--url ...]                             │
│    → creates / updates the shared .ai/src/mcp.json                            │
│    → applied to every enabled tool with a compatible MCP target               │
│                                                                               │
│  exuno customize <tool> <resource>                                        │
│    → for the rare case you need a per-tool override that differs from         │
│      the shared MCP map, or to materialize a payload enable --no-scaffold     │
│      skipped (`customize cursor hooks`)                                       │
└───────────────────────────────────────────────────────────────────────────────┘
```

**Mental model:**

- `enable` is the entry point. One command turns a tool on _and_ gives you the file to edit.
- Shared MCP is the default. `add mcp` writes canonical `mcpServers` to `.ai/src/mcp.json`; OpenCode converts that map and composes it with its settings atomically.
- `customize` is the escape hatch. Use it only when you need a per-tool override that diverges from the shared source, or when `enable --no-scaffold` skipped materializing a file you later want.

All three write to **`.ai/src/tools/<tool>/`** (per-tool) or **`.ai/src/mcp.json`** (shared). Nothing is scattered across `.ai/src/hooks/`, `.ai/src/mcp/`, `.ai/src/settings/` — the old flat layout is kept around for backward compatibility (see [migrate](#migrating-from-the-010-flat-layout)).

## How Resources Resolve

Every payload resource — tool YAML, hooks, MCP config, settings — follows the same layered lookup. Nothing is cloned into every project by default; overrides are opt-in and merge on top of a shipped base.

```
┌─ 1. Per-tool override ─────────┐   ┌─ 2. Shared MCP (mcp only) ──────┐   ┌─ 3. Shipped base ──────────────────────┐
│ .ai/src/tools/<tool>/          │   │ .ai/src/mcp.json                │   │ embedded lib/templates/<resource>/     │
│   <resource>.<ext>             │►► │ (compatible MCP targets)        │►► │   <tool>.<ext>                         │
└────────────────────────────────┘   └─────────────────────────────────┘   └────────────────────────────────────────┘

     per-tool wins   →   shared fills in (for MCP)   →   base fills in otherwise
```

| Resource  | Base path                             | Per-tool override                     | Shared override    |
| --------- | ------------------------------------- | ------------------------------------- | ------------------ |
| tool YAML | `lib/templates/tools/<tool>.yaml`     | `.ai/src/tools/<tool>.yaml`           | —                  |
| hooks     | `lib/templates/hooks/<tool>.<ext>`    | `.ai/src/tools/<tool>/hooks.<ext>`    | —                  |
| mcp       | `lib/templates/mcp/<tool>.json`       | `.ai/src/tools/<tool>/mcp.json`       | `.ai/src/mcp.json` |
| settings  | `lib/templates/settings/<tool>.<ext>` | `.ai/src/tools/<tool>/settings.<ext>` | —                  |

The legacy flat-layout overrides (`.ai/src/hooks/<tool>.<ext>`, `.ai/src/mcp/<tool>.<ext>`, `.ai/src/settings/<tool>.<ext>`) from 0.10 and earlier are still read and still win over base, but print a one-shot deprecation warning. Run `exuno migrate --legacy` to preview moving them into the canonical per-tool layout, then `exuno migrate --apply` to apply it; the legacy paths may be dropped in a future release.

**Why it matters:**

- **Lean by default.** `exuno init` creates `.ai/exuno.yaml`, `AGENTS.md`, and your chosen content sections — no pre-written hooks / MCP / settings for 15 tools you don't use.
- **Updates flow through.** Because the base ships with the engine, `exuno update` improves every project that hasn't locked the file in as an override.
- **Shared MCP is converted where schemas differ.** One `.ai/src/mcp.json` reaches every enabled MCP target. OpenCode's adapter validates local and remote transports, then atomically composes the result into `opencode.json`.
- **MiniMax Code MCP uses the project `.mcp.json`.** Claude Code shares that destination. When their effective MCP sources differ, sync stops before writing either version. MiniMax may start a configured server during tool discovery or use, so review an MCP source before syncing it.
- **Global settings files are shared with the tools that read them.** Synced from `$HOME` or into a profile, Exuno owns only the keys `.ai/src` declares in each TOML or JSON settings and MCP file, one entry per MCP server, and keeps what the tool writes there itself: Codex's project trust and plugins, Claude Code's model, theme, and plugin choices, a server added in Cursor. A declared key the tool changed stops the sync by name; `exuno adopt <file>` pulls it into the source. `targets.settings.ownership` and `targets.mcp.ownership` (`auto`, `keys`, `file`) override the choice; in a repository `auto` owns files whole, and Zed's shipped tool owns its commented settings whole.
- **Shared `AGENTS.md` needs one source of truth.** If enabled tools read different agents content for the same destination, sync stops before either output is written.
- **MiniMax Code has no config-home profile.** It reads files from the primary project workspace, so `profile add` and `sync` refuse MiniMax profile variants instead of creating files the client would not load.
- **Opt in per tool.** Need to edit Cursor's hooks? `exuno customize cursor hooks` copies the current base into `.ai/src/tools/cursor/hooks.json`. Delete the file later to resume inheriting.
- **Safe hooks.** `customize <tool> hooks` prints the base content first and requires `--yes` in non-interactive mode — you never scaffold executable intent silently.
- **`simplify` prunes noise.** Scaffolded payloads that are still byte-identical to base are flagged by `exuno simplify` and removed with `--apply`, so you don't accidentally pin yesterday's defaults forever.

## Migrating from the 0.10 flat layout

Projects upgraded from 0.10 keep working without intervention — the resolver still reads `.ai/src/{hooks,mcp,settings}/<tool>.<ext>`. When you are ready to move them into the canonical per-tool layout:

```bash
exuno migrate --legacy    # dry-run; prints the planned legacy-layout moves
exuno migrate --apply     # performs the moves; consolidates identical MCP files
exuno migrate --apply -y  # non-interactive — accepts MCP consolidation by default
```

`migrate` moves each legacy file to `.ai/src/tools/<tool>/<resource>.<ext>`. When every `.ai/src/mcp/*.json` is byte-identical, it offers to collapse them into the shared `.ai/src/mcp.json`; if they differ, they migrate per-tool. Existing files at the target are never overwritten — such collisions are skipped with a warning so you resolve them by hand. Empty source directories are cleaned up on success.

`migrate` also detects the pre-v0.6 monolithic `.agent/` (singular, no `s`) layout — a single directory holding `AGENTS.md`, `workflows/`, `rules/`, `skills/` without per-tool separation. The current engine doesn't recognise it, so a normal `sync` never cleans it up. Dry-run lists the contents; `--apply --yes` removes the directory. Detection runs alongside the flat-layout move logic, so a single `migrate --apply --yes` cleans up both in one pass.

## Migrating an outdated Exuno project

Run `exuno migrate` from the project root. It prints a self-contained prompt
and automatically copies it with `pbcopy`, `wl-copy`, `xclip`, `xsel`, or
`clip.exe`, whichever is available. Paste the prompt into a coding AI that can
inspect the repository.

The prompt includes the installed CLI version and the project's
`exuno_version` pin. It directs the AI to verify the actual starting version,
read every relevant official changelog section, consult the latest README,
bundled skill, and templates from the same stable release, create a recoverable
checkpoint, preserve custom configuration, use supported migration commands,
and finish with `doctor`, `sync`, and `check`. If no clipboard integration is
available, the prompt is still printed to stdout.

## Path Overrides

Create `exuno.yaml` in the project root to override source paths. Relative
values are resolved from the project root; absolute values may point at a
separately maintained source tree. `source.tools` controls both the per-tool
YAML files and their payload directories (`<source.tools>/<tool>/settings.json`,
`hooks.json`, or `mcp.json`), so those files stay on the same source side:

```yaml
outputs: committed # or local — see "Where generated files live"

source:
  agents: ".ai/src/AGENTS.md"
  rules: ".ai/src/rules"
  skills: ".ai/src/skills"
  tools: ".ai/src/tools"
```

For a user-wide source tree, keep outputs in the project (or use the tool
configuration's supported project-relative destinations) and point only the
sources outward:

```yaml
source:
  agents: "/home/me/agentic/.ai/AGENTS.md"
  rules: "/home/me/agentic/.ai/rules"
  skills: "/home/me/agentic/.ai/skills"
  tools: "/home/me/agentic/.ai/tools"
```

Then trust that tree from outside the repository, in your shell profile or CI
environment:

```bash
export EXUNO_EXTERNAL_SOURCE_ROOTS="/home/me/agentic"   # colon-separated
```

Sources outside the project follow these rules:

- **Trusted roots only.** A value outside the project must resolve under an
  absolute directory listed in `EXUNO_EXTERNAL_SOURCE_ROOTS`; otherwise
  `sync` and `check` stop before writing, and `doctor` reports it. Only the
  environment grants that trust, never `exuno.yaml`, so syncing a cloned
  repository — including the `shell-init` hook on `cd` — cannot read your
  files from wherever its config points.
- **Explicit values only.** A `source.agents`, `source.rules`, `source.skills`,
  `source.commands`, `source.subagents`, or `source.tools` value written in the
  selected project config may point outside the project, as an absolute path
  or a `../` path. Nothing else widens where sources are read from: the
  install-dir defaults, the auto-detected `.ai/src/` and `.ai/` layouts, and any
  value written under the project keep the project boundary.
- **Symlinks follow the same rule.** Before reading anything, `sync` and
  `check` resolve every symlink under `.ai/` and the configured sources,
  following chains and links to directories. One whose target is outside the
  project and not under `EXUNO_EXTERNAL_SOURCE_ROOTS` — a committed
  `.ai/src/rules/notes.md -> ~/secrets.md`, say — stops the run before
  writing, naming the link. A link to a shared tree you maintain works once
  that tree is listed in the variable.
- **Refused roots.** A value that resolves to `/`, your home directory, the
  project root, or a directory containing the project root is rejected before
  anything is written; `doctor` reports it for every key except `tools`.
- **Relative to the project root.** Relative values resolve from the project
  root, including when `EXUNO_CONFIG_PATH` selects a config file stored
  elsewhere and inside `check`'s temporary workspace.
- **Read-only.** Outside sources are only read. Destinations stay confined to
  the project root, and `check` reads the sources in place while generating
  only in its temporary workspace.
- **No writes into an outside tool catalog.** When `source.tools` resolves
  outside the project, `customize`, `profile add`/`remove`, `adopt`,
  `simplify --apply`, `enable --scaffold`, and a `disable` that would flip a
  legacy `enabled: true` exit with an error before writing; plain `enable`
  skips payload scaffolding. Edit that catalog where it lives.

`EXUNO_CONFIG_PATH` selects an alternate configuration file. Every command
that reads the project config fails when it names a missing file instead of
falling back to `.ai/exuno.yaml`.

## Migrating Existing Configurations

If you already have tool-specific configs (`.claude/rules/`, `.cursor/rules/`, custom `settings.json`, etc.), **move them into `.ai/src/` before running `exuno sync`**. Sync treats generated directories as fully managed — any files not present in the source will be overwritten or removed.

### Step-by-step

1. **Run `exuno init`** to create the `.ai/src/` structure (skips files that already exist).

2. **Move your rules** from tool-specific directories into `.ai/src/rules/`:

   ```bash
   # Example: you had custom Cursor rules
   mv .cursor/rules/my-api-conventions.mdc .ai/src/rules/my-api-conventions.md
   # Remove Cursor-specific frontmatter (---/globs/alwaysApply) — Exuno adds it automatically

   # Example: you had custom Claude rules
   mv .claude/rules/testing.md .ai/src/rules/testing.md
   ```

3. **Move your skills** into `.ai/src/skills/`:

   ```bash
   mv .claude/skills/my-skill/ .ai/src/skills/my-skill/
   ```

4. **Move your commands** into `.ai/src/commands/`:

   ```bash
   mv .claude/commands/deploy.md .ai/src/commands/deploy.md
   ```

5. **Move your agents** into `.ai/src/agents/`:

   ```bash
   mv .claude/agents/security-auditor.md .ai/src/agents/security-auditor.md
   ```

6. **Move settings, MCP, and hooks** into `.ai/src/settings/`, `.ai/src/mcp/`, `.ai/src/hooks/`:

   ```bash
   mv .claude/settings.json .ai/src/settings/claude.json
   mv .mcp.json .ai/src/mcp/claude.json
   mv .cursor/mcp.json .ai/src/mcp/cursor.json
   ```

7. **Run sync** and verify:

   ```bash
   exuno sync --dry-run   # Preview what will be generated
   exuno sync              # Apply
   ```

### What gets overwritten

| Target                                                           | Behavior                                                                                                                                  |
| ---------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------- |
| **Rules directories** (`.claude/rules/`, `.cursor/rules/`, etc.) | Files matching the synced extension (`.md`, `.mdc`, `.instructions.md`) are managed by sync. Files not present in source are **removed**. |
| **AGENTS/CLAUDE.md/GEMINI.md**                                   | **Fully replaced** from `.ai/src/AGENTS.md` on every sync.                                                                                |
| **settings.json, .mcp.json, hooks.json**                         | **Fully replaced** from their respective source files.                                                                                    |
| **Skills, commands, agents directories**                         | Synced contents replace existing files. Extra files are **removed**.                                                                      |
| **.gitignore**                                                   | Only the `AI SYNC GENERATED START/END` block is managed. Your other entries are safe.                                                     |

### Transactional backups and rollback

Before a real `init` or `sync` writes anything, Exuno copies every path the operation may change into `.ai/backups/<backup-id>/`. The backup store ignores itself in Git. Dry runs, fresh `sync --if-stale` calls, and syncs rejected by drift preflight do not create snapshots.

If an operation exits with an error, Exuno automatically restores its pre-operation snapshot, including paths that did not exist before the run. Successful snapshots remain available for an accidental-sync rollback:

```bash
exuno rollback --list                 # show complete snapshots
exuno rollback --dry-run              # preview the latest snapshot
exuno rollback                        # restore latest (interactive)
exuno rollback <backup-id> --yes       # restore a selected snapshot
```

Rollback creates its own safety snapshot first, so its output prints an ID that can undo the rollback. After each operation — successful, or failed once its restore completes — Exuno prunes the history: a snapshot is kept only if it is among the latest 10 **and** younger than 30 days. Set `EXUNO_BACKUP_LIMIT` or `EXUNO_BACKUP_MAX_AGE_DAYS` to another non-negative integer to change either bound, or to `0` to disable that bound alone. The newest snapshot is always retained, so rollback stays available however long a project sits idle. Staging directories left behind by an interrupted or killed run are reclaimed on the next backup, once they are over 24 hours old.

To retain all existing recovery data automatically, set this in the project's
`.ai/exuno.yaml` (or root `exuno.yaml`, or the file selected by
`EXUNO_CONFIG_PATH`):

```yaml
backup:
  retention: preserve
```

`backup.retention` accepts `bounded` (the default behavior above) or `preserve`.
Preserve disables both snapshot pruning and the cleanup of existing
`.tmp.*`, `.latest.tmp.*`, and `.gitignore.tmp.*` staging entries, regardless
of the count and age bounds. Setting both environment bounds to `0` alone
does **not** disable staging cleanup. With preserve, disk usage can grow
without a bound; review and remove recovery data manually when appropriate.

Init, sync, and rollback validate the policy and numeric bounds before changing
targets or the backup store; `rollback --list` and `check`, which change
neither, skip that validation. An explicitly empty or unknown retention value is
an error. The selected policy stays fixed for the operation, including automatic
recovery after a failed sync and a rollback that restores a different config.
New snapshots are still created and the current `.latest` pointer and
`.gitignore` metadata are still updated. Temporary files created by the current
operation can still be cleaned up on failure; preserve protects recovery that
already existed when the operation began.

The transaction covers declared tool destinations plus `.ai/.sync-manifest` and the managed `.gitignore` state. A trusted `post_sync` hook can execute arbitrary commands; side effects it makes outside those paths are outside Exuno's rollback boundary.

Rollback first compares every target with the state recorded **after** the
selected operation. Later additions, edits, deletions, type or executable-bit
changes, and foreign children inside a directory — even a `.DS_Store` — cause
the entire rollback to abort before any target is restored, naming the first
differing path. This also applies to declared native or disabled destinations:
a file recorded as absent is not permission to delete a file someone created
later. `--yes` skips confirmation only; it does not bypass conflict checks.
`--dry-run` shows the plan together with the conflict and exits 1.
`rollback --force` skips the check and restores anyway, still creating the
safety snapshot that can undo it.

Rolling back an older snapshot after newer init, sync, or rollback runs is a
conflict whenever those runs changed its targets. Roll back the newer snapshots
first, newest to oldest, or use `--force` to jump straight to the older state.

New snapshots receive an `after.tsv` record once init, sync, or rollback has
finished: one line per file, directory, symlink, or missing target, with
content hashes and the executable bit, bound to the snapshot's target list.
Undo uses the result of the preceding rollback as its expected state. If the
record cannot be written — no `sha256sum` or `shasum`, an unreadable file — the
operation still succeeds with a warning. Snapshots without a usable record
(taken before this check existed, left unsealed by such a warning, or with an
empty or damaged `after.tsv`) are restored as before, with a warning that
changes made after their operation cannot be detected.

Symlinks are compared by their link text, without inspecting or restoring their
destination contents. Changes to mutable knowledge behind an unchanged link
therefore neither block rollback nor get reverted. Replacing a link itself is
a conflict. A target reached through a symlinked parent directory is compared
through it while the link resolves inside the project; one resolving outside
is refused.

The preflight runs once before the plan and again after creating the safety
snapshot, right before enabling restore/recovery. These checks are not an atomic
filesystem transaction or a lock: concurrent writers can still change files or
links during a scan or between the final check and a write (TOCTOU). Stop other
writers while syncing or rolling back. Internal recovery of a failing operation
remains separate from the guarded user-requested rollback; the snapshot witness
does not authenticate data against an actor who can rewrite the backup store.

### Drift detection

After every successful sync, Exuno writes `.ai/.sync-manifest` — one line per generated file with its SHA-256 hash. It records what *this clone* generated, so sync adds it to the managed `.gitignore` block next to the outputs it describes: a committed manifest beside ignored outputs would make every teammate's next sync read a `git pull` as manual edits. On the next sync, every destination is compared against the manifest:

- File untouched → sync rewrites silently (idempotent).
- File deleted manually → sync rewrites silently.
- File **edited** since last sync → sync **aborts** with the list of edited paths and your unsynced changes are preserved.

```text
[ERROR] Manual edits detected in 1 destination file(s) since last sync:
      .claude/rules/core.md

  These files would be silently overwritten. Choose one:
    • Move your edits into .ai/src/, then re-run sync
    • If a tool wrote here out of band, run 'exuno adopt <file>' to pull it into .ai/src/
    • Re-run with --force to discard the edits and rewrite from source
```

`exuno check` and `exuno doctor` both surface drift, so a stale or hand-edited clone is visible before the edit is lost.

### `exuno adopt` — promote an IDE edit back into source

Quickly iterating in Claude Code or Cursor and edited a generated file directly? Skip the manual `cp + sync --force` dance:

```bash
exuno adopt .claude/rules/core.md            # interactive, with diff preview
exuno adopt --dry-run .claude/rules/core.md  # show plan, write nothing
exuno adopt --yes .claude/rules/core.md      # non-interactive (CI / scripts)
exuno adopt --all                            # adopt every drifted file at once
```

Resolves the destination back to its source file (`.ai/src/rules/core.md`), copies the edited content, and refreshes the manifest entry — the next `sync` is drift-free.

`--all` scans the manifest for every drifted (manually-edited) output and adopts them in one pass. Refused targets (below) are skipped and listed. If two edited outputs resolve to the same source with different content (e.g. `CLAUDE.md` and `GEMINI.md` both map back to `.ai/src/AGENTS.md`), both are skipped so neither silently clobbers the other — adopt one explicitly.

**Refused targets** (the round-trip would corrupt your source):

- Rules that get a frontmatter header (`cursor`, `copilot`, `windsurf`, `antigravity`) — adopting would push that tool's header into every other tool's rules.
- Rules merged into a single file (`zed`) — many sources collapsed into one dest can't be split back apart.
- Rules or skills inlined into AGENTS.md (rules: `codex`, `gemini`, `junie`, `kimi`, `minimax`, `opencode`; skills: `amazonq`, `cline`, `zed`).
- Format-converted output (`codex` subagents → TOML, `amazonq` subagents → JSON, `opencode` subagents → OpenCode Markdown).

For these, edit `.ai/src/` directly. Exuno names the offending file when it refuses.

### Disabling sync for tools or categories

If you want to keep managing a tool manually, disable it in its YAML config:

```yaml
# .ai/src/tools/cursor.yaml
enabled: false
```

Or exclude it at sync time:

```bash
exuno sync --skip cursor
```

To keep the tool enabled but omit one output category, set that target's
`enabled` flag to `false`. All other categories continue to sync normally:

```yaml
# .ai/src/tools/claude-hub.yaml
base: claude
targets:
  rules:
    enabled: false
```

This works for every entry under `targets:` and is useful when a `base:`
variant inherits a destination that another config should own.

### Letting a tool read `AGENTS.md` instead of its own file

Exuno already writes the root `AGENTS.md` as the instructions file for four
tools: Codex, Cursor, OpenCode, and Windsurf. Claude Code can join them, because
it reads `AGENTS.md` in a folder that has no `CLAUDE.md`. A project that would
rather ship one instructions file than two turns the second one off:

```yaml
# .ai/src/tools/claude.yaml
targets:
  agents:
    enabled: false
```

```bash
rm CLAUDE.md          # only if a previous sync already wrote it
exuno sync
```

Everything else Claude Code gets stays: `.claude/rules/`, skills, commands,
subagents, settings, hooks, and MCP are separate targets and are untouched.

Delete the file as well as disabling the target. A `CLAUDE.md` left from an
earlier sync keeps being read by Claude Code, and because nothing regenerates
it, it quietly ages. Sync leaves it in place, so a file you froze on purpose
survives; `exuno check` and `exuno doctor` name it as left from a
disabled target without failing. Once it is gone, the next sync drops it from
the manifest.

## Workspaces — nested Exuno projects

A parent project at `workspace/.ai/src/` with sub-projects below (`workspace/foo/.ai/src/`, `workspace/bar/.ai/src/`) is supported as a first-class workflow. Two patterns to manage shared content between the layers:

**Pattern A — declarative inheritance via `shared:`.** Each child project declares which categories it inherits from the parent. At sync time, Exuno builds a transient shadow `.ai/src/` (child + parent fillers, child wins on collisions) and materialises the result into every enabled tool's output — works for tools without parent-loading (Codex, Cursor, JetBrains Junie) and tools with it (Claude Code) equally. Inherited files are never written into the child's `.ai/src/`; they live only in the shadow tree during a single sync run.

```yaml
# child/.ai/exuno.yaml
shared:
  path: "../"
  inherit: rules,skills,commands,agents
```

**Pattern B — interactive cleanup via `dedupe`.** When child and parent both have the same source file (a copy-paste duplicate), `dedupe` compares them by hash:

```bash
exuno dedupe                  # walk up to nearest parent .ai/src/ (bounded by git boundary)
exuno dedupe --against ../    # explicit parent path
exuno dedupe --workspace      # bottom-up alphabetical fan-out across every nested .ai/
exuno dedupe --yes            # non-interactive: delete identical-hash dupes, leave divergent
```

Identical-hash files become a `[d]elete / [k]eep / [v]iew` prompt; for shipped templates the deletion also writes a `template_overrides.declined` entry so `refresh` won't re-offer the file. Divergent files (same path, different content) show a diff and leave the decision to the human — dedupe never auto-resolves a divergence.

**Detection — `exuno doctor`.** Doctor walks up to the nearest parent `.ai/src/` (same boundary as dedupe) and flags identical-hash duplicates as advisories and divergent files as info. Rules and skills marked with `category: governance` in their frontmatter are upgraded to advisories when divergent, with explicit "likely a mistake, not an override" framing. All cross-project findings are exit-code-0 advisories — visible during interactive runs, invisible to CI gates, so pre-commit hooks running `doctor` don't break on workspace techdebt.

**Workspace-wide sync.** `exuno sync --workspace` runs `sync` in every Exuno-managed `.ai/` below cwd, bottom-up alphabetical (deeper paths first; siblings sorted by `LC_ALL=C` for reproducibility). Continues past per-project failures; reports max exit code at the end. All other sync options (`--only`, `--skip`, `--profile`, `--dry-run`, `--force`, `--quiet`, `--json`) forward to each per-project invocation, so `--json` prints one object per project.

The walk-up logic stops at the start's git repository boundary, so a child project with its own `.git` never picks up an unrelated parent `.ai/src/` from above the boundary.

## Profiles — multiple config homes per tool

Sync from `$HOME` and juggle more than one account for the same tool — a work Claude and a personal Claude? A **profile** generates a second, self-contained config home (`~/.claude-hub/` next to your personal `~/.claude/`) whose content is the base `.ai/src/` plus profile-only extras. Shared rules stay shared; work-only rules and MCP servers live in the profile.

```bash
exuno profile add hub                       # scaffold a "hub" profile for every enabled tool
exuno profile add hub --tools claude,codex  # ...or just these tools
exuno profile add hub --adopt               # pull an existing ~/.claude-hub/ into the profile first
exuno profile list                          # show profiles, their tools, and config homes
exuno profile remove hub                    # delete the config-home output and the profile
```

`profile add` writes three things: a thin variant tool `.ai/src/tools/<tool>-hub.yaml` that inherits everything from the base tool via `base:` and only overrides the dest paths, an overlay directory `.ai/profiles/hub/src/` for profile-only content (rules, skills, commands, agents, AGENTS.md — profile wins on collisions), and a `profiles:` block in `exuno.yaml`.

On sync, `exuno sync` builds every `active` profile alongside your personal tools; `exuno sync --profile hub` builds just that one. The per-profile overlay layers over the base — and over an active `shared:` overlay if you have one, so the two compose. Run the result with the tool's config-home variable, for example `CLAUDE_CONFIG_DIR=~/.claude-hub claude`. Profile outputs are gitignored and drift-protected like any other generated file.

## Development

Exuno is a Rust crate at the repository root (edition 2024, `unsafe_code`
forbidden); `src/main.rs` is the `exuno` binary and the tool templates in
`lib/templates/` are embedded into it at build time. Configuration is read by
`src/config/yaml_subset.rs`, a parser for the YAML shapes Exuno accepts, so the
binary has no YAML dependency. Build it and run the checks from the repo root:

```bash
cargo build --release                             # target/release/exuno
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo test                                        # the whole suite

# A single command surface:
cargo test --test sync
```

`cargo test` is everything: unit tests inside `src/` and integration tests in
`tests/*.rs`, one file per command surface, each driving the binary Cargo just
built. `tests/common/mod.rs` is the shared harness — a throwaway git project in
a temp directory, the binary with your `EXUNO_*` and `AGENTSYNC_*` variables and git config
scrubbed out, and the file helpers the assertions need. CI runs the same gates
on Linux, macOS, and Windows. `shellcheck -x -S warning -e SC1091` covers the
two shell scripts that remain: `install.sh` and
`lib/templates/guard/claude.sh`.

The engine was ported from Bash one command at a time
(`docs/specs/2026-09-12-rust-migration-design.md`). The Bash engine last
shipped in 0.37.0 and is readable from that tag, for example
`git show 0.37.0:lib/helpers/backup.sh`; the module docs in `src/` name the
Bash function each file mirrors. `scripts/perf/bench.sh` times the binary on a
generated 13-tool fixture, and compares it against the Bash engine when
`AGENTSYNC_BASH_CLI` points at the `bin/agentsync.sh` of a 0.37.0 checkout.

Releases are built by cargo-dist (`dist-workspace.toml`): the auto-tag
workflow dispatches `release.yml` for the tag it creates from `VERSION`, which
publishes the five archives, their checksums, and the installers.

## License

Copyright (C) 2026 Yelaman Yelmurat.

Exuno is free software licensed under the
[GNU General Public License version 3 only](LICENSE) (`GPL-3.0-only`).
You may use, modify, and redistribute it under that license's terms. Distributed
modified versions must remain under the same license and provide corresponding
source code. Third-party components retain their original licenses; see
[Third-Party Notices](THIRD_PARTY_NOTICES.md).

## Uninstall

```bash
# Global
rm -rf ~/.exuno && rm -f /usr/local/bin/exuno /usr/local/bin/agentsync
# Remove AGENTSYNC_HOME from ~/.zshrc if a source install added it

# Per project
rm -rf .ai/
# Remove AI SYNC GENERATED block from .gitignore
```

<!-- ---

## Star history

<a href="https://star-history.com/#yelmuratoff/exuno&Date">
  <img src="https://api.star-history.com/svg?repos=yelmuratoff/exuno&type=Date" alt="Star History Chart">
</a>

<div align="center">
  <a href="https://github.com/yelmuratoff/exuno/graphs/contributors">
    <img src="https://contrib.rocks/image?repo=yelmuratoff/exuno" />
  </a>
</div> -->
