<div align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://raw.githubusercontent.com/yelmuratoff/exuno/main/assets/exuno.svg">
    <img src="https://raw.githubusercontent.com/yelmuratoff/exuno/main/assets/exuno_light.svg" alt="Exuno" width="400">
  </picture>

  <h3>One source → 15 AI tools. Stop copy-pasting skills</h3>

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

Exuno keeps your AI agent instructions in one folder, `.ai/src/`, and writes them out in the format each coding tool expects. It supports 15 tools: Claude Code, GitHub Copilot, Cursor, Gemini CLI, OpenAI Codex, Kimi Code, MiniMax Code, OpenCode, Devin Desktop (Windsurf), JetBrains Junie, Cline, Kiro, Amazon Q, Zed, and Google Antigravity.

## Why you might want it

Every AI coding tool reads instructions from its own place and in its own format: `.claude/rules/*.md`, `.cursor/rules/*.mdc`, `.github/instructions/*.instructions.md`, `AGENTS.md`, `.devin/rules/`, and so on. Once a project uses two tools, or teammates use different ones, the same rules live in several copies. Someone updates one copy and forgets the others, and the copies drift apart.

With Exuno you keep one copy. You edit `.ai/src/`, run `exuno sync`, and every tool you enabled gets its own files.

## How it works

Here is what one rule turns into:

```
.ai/src/rules/testing.md
    ↓ exuno sync
├── .claude/rules/testing.md                      # plus an @rules/testing.md import in CLAUDE.md
├── .cursor/rules/testing.mdc                     # plus globs/alwaysApply frontmatter
├── .github/instructions/testing.instructions.md  # plus applyTo frontmatter
├── .devin/rules/testing.md                       # plus trigger: always_on frontmatter
├── .amazonq/rules/testing.md
├── AGENTS.md                                     # a short reference (Codex, Gemini, Junie, Kimi, MiniMax, OpenCode)
└── .rules                                        # merged into one file (Zed)
```

A sync goes through these steps:

1. **Read the project.** Exuno reads `.ai/exuno.yaml` (which tools are on, where the sources are) and the files under `.ai/src/`.
2. **Work out each tool's config.** Every tool is described by a YAML file that ships inside the binary: where its files go, which extension they get, what header they need. If you put a file with the same name in `.ai/src/tools/`, your values replace the shipped ones.
3. **Stop if someone hand-edited a generated file.** Exuno remembers the hash of every file it wrote in `.ai/.sync-manifest`. If a generated file changed since the last sync, it stops and tells you which one, so the edit isn't lost.
4. **Back up, then write.** Exuno copies everything it is about to change into `.ai/backups/`, then writes each tool's files. If anything fails, it puts the old files back. `exuno rollback` undoes a sync that worked but that you didn't want.
5. **Record what it wrote.** Exuno updates the manifest and its block in `.gitignore`, and removes the files of tools you turned off.

Rules apply everywhere by default. To make a rule apply only to certain files, give it `paths:` frontmatter with a list of globs. Exuno then writes each tool's own way of saying "only for these files" (`alwaysApply: false` for Cursor, `applyTo:` for Copilot, `trigger: glob` for Windsurf; Claude keeps `paths:`), so the agent loads the rule only when it works on a matching file.

The full walk-through, including how shipped defaults and your overrides combine, is in [How sync works](docs/guide/how-sync-works.md).

## Install

You need `curl` and `tar`. Exuno is one static binary for macOS (Apple silicon and Intel), Linux (x86_64 and arm64), and Windows (x86_64).

```bash
curl -fsSL https://raw.githubusercontent.com/yelmuratoff/exuno/main/install.sh | bash
```

On Windows, from PowerShell:

```powershell
irm https://github.com/yelmuratoff/exuno/releases/latest/download/exuno-installer.ps1 | iex
```

The installer downloads the release for your platform, checks its sha256, puts the binary at `~/.exuno/bin/exuno`, and links `exuno` into `/usr/local/bin/` (or `~/.local/bin/` if it can't write there).

To install a specific release, for example the one your project pins, set `EXUNO_VERSION`:

```bash
EXUNO_VERSION=0.45.0 curl -fsSL https://raw.githubusercontent.com/yelmuratoff/exuno/main/install.sh | bash
```

Later, `exuno update` moves you to the latest release and `exuno update <version>` to a specific one.

Exuno was called AgentSync up to 0.44, and the old names keep working until 1.0. To rename things in an existing project, see [Coming from AgentSync](docs/guide/migrating.md#coming-from-agentsync). Older releases and source installs are covered in the [migration guide](docs/guide/migrating.md#older-releases-and-source-installs).

## Quick start

```bash
cd your-project
exuno init                  # set up .ai/, keep the config you already have, run the first sync
exuno enable claude cursor  # turn on the tools you use
# edit files in .ai/src/
exuno sync                  # write every enabled tool's files
```

`exuno init` asks a few questions in a terminal: which tools, which kinds of content, whether to commit the generated files, and whether to add a CI check. In a script it picks defaults and detects tools from folders that already exist (`.claude/`, `.cursor/`, `CLAUDE.md`, ...). It keeps the tool config the project already has, so running it in an existing project is safe. Running it twice is safe too; the second run does nothing.

After the first sync, the tool folders appear (`.claude/`, `.cursor/`, `.github/`, `.devin/`, and so on), each with files in that tool's format.

From then on, edit only `.ai/src/`. A sync rewrites the generated folders from the source. If you change a generated file by hand, the next sync stops and asks what to do; `exuno adopt <file>` copies your edit back into `.ai/src/`.

A few more things you can do:

```bash
exuno add rule testing                      # new rule from a template
exuno add skill deploy                      # new skill
exuno add mcp github --command …            # share an MCP server with every tool that supports MCP
exuno generate "React + Next.js" | pbcopy   # a prompt you paste into a chat AI to draft .ai/src/ for your stack
exuno check                                 # exit 1 if generated files don't match the source
```

## Working in a team

One person sets up the project. Everyone else just pulls.

```bash
cd your-project
exuno init --tools claude,cursor
git add -A && git commit -m "chore: add exuno"
```

By default the generated files are committed. That means:

- Teammates don't need Exuno at all. `git pull` brings `CLAUDE.md`, `.claude/rules/`, `.cursor/rules/`, and the rest, already up to date.
- Whoever changes the rules edits `.ai/src/`, runs `exuno sync`, and commits the source and the generated files together. `exuno setup-hooks` adds a pre-commit hook that enforces this, and `exuno check` in CI catches anyone who skips the hook. When the project has a `.github/` folder, `init` offers to add that CI job.
- Agents are told to edit `.ai/src/` too. The shipped rules say so, and Claude Code gets a hook that blocks writes to generated files and names the source file to edit instead.

`init` also pins the Exuno version in `.ai/exuno.yaml`. Every machine and CI then produce the same bytes, and `sync` and `check` stop if the installed version doesn't match.

If you'd rather not commit generated files, use `exuno init --outputs local`. Then everyone needs Exuno installed and should run `exuno setup-hooks`, which syncs after each pull. Both modes are compared in [Team setup and automation](docs/guide/team-and-automation.md).

## What goes in `.ai/src/`

```
.ai/
├── exuno.yaml          # project config: enabled tools, version pin, options
└── src/                # the source; edit only here
    ├── AGENTS.md       # who the agent is: role, approach, principles
    ├── rules/          # constraints, one file per topic
    ├── skills/         # step-by-step recipes the agent loads when it needs them
    ├── commands/       # custom slash commands
    ├── agents/         # subagent personas
    ├── mcp.json        # MCP servers shared by every tool that supports MCP
    └── tools/          # per-tool overrides (optional)
        ├── claude.yaml #   change where or how Claude's files are written
        └── claude/     #   Claude's own settings.json, hooks.json, mcp.json
```

`init` creates only `exuno.yaml`, `AGENTS.md`, and the content folders you picked. Settings, hooks, and MCP files for each tool come from defaults built into Exuno. You get an editable copy of one only when you ask for it with `exuno enable`, `exuno customize`, or `exuno add mcp`.

| In `.ai/src/` | What it is | What tools receive |
| --- | --- | --- |
| `AGENTS.md` | The agent's role and principles | Copied under each tool's name: `CLAUDE.md`, `GEMINI.md`, `.junie/AGENTS.md`, `AGENTS.md`, ... |
| `rules/` | Constraints the agent always follows, or only for files matching `paths:` | Separate files with each tool's header, a merged file (Zed), or a short reference list in `AGENTS.md` |
| `skills/` | Recipes in the [Agent Skills](https://agentskills.io) format: a folder with `SKILL.md` | Skill folders for every tool that supports them, otherwise an index in `AGENTS.md` |
| `commands/` | Slash commands such as `/review` | Native commands where they exist, TOML for Gemini, generated skills for Codex, Kimi, and Kiro, an index for Amazon Q and Zed |
| `agents/` | Subagents with their own context and tool limits | Converted to each tool's agent format |
| `mcp.json` | Shared `mcpServers` map | Each tool's MCP file, converted where the format differs |
| `tools/` | Overrides for one tool | Not copied; changes how that tool is synced |

[Supported tools](docs/guide/tools.md) has the full table of what each tool gets and every format conversion. Skills have their own page: [Skills](docs/guide/skills.md).

## Supported tools

| Tool | Main instructions file |
| --- | --- |
| Claude Code | `CLAUDE.md` |
| GitHub Copilot | `.github/copilot-instructions.md` |
| Cursor | `AGENTS.md` |
| Gemini CLI | `GEMINI.md` |
| OpenAI Codex | `AGENTS.md` |
| Kimi Code | `.kimi-code/AGENTS.md` |
| MiniMax Code | `AGENTS.md` |
| OpenCode | `AGENTS.md` |
| Devin Desktop (Windsurf) | `AGENTS.md` |
| JetBrains Junie | `.junie/AGENTS.md` |
| Cline | `AGENTS.md` |
| Kiro | `AGENTS.md` |
| Amazon Q | `.amazonq/rules/00-context.md` |
| Zed | `.rules` |
| Google Antigravity | `GEMINI.md` |

Turn tools on with `exuno enable <tool>` and see their status with `exuno list`. Rules, skills, commands, subagents, settings, MCP, and hooks are synced wherever the tool supports them; the per-tool details are in [Supported tools](docs/guide/tools.md). A tool Exuno doesn't ship is one YAML file in your project; see [Adding a new tool](docs/guide/configuration.md#adding-a-new-tool).

## Commands

| Command | What it does |
| --- | --- |
| `init` | Set up `.ai/` in a project and run the first sync |
| `sync` | Write every enabled tool's files from `.ai/src/` |
| `check` | Exit 1 if generated files don't match the source (for CI) |
| `enable <tools…>` / `disable <tools…>` | Turn tools on or off |
| `list` (`ls`) | Show tools and their status |
| `add <kind> <name>` | Create a `rule`, `skill`, `command`, `subagent`, or `mcp` server |
| `adopt <file>` | Copy a hand edit in a generated file back into `.ai/src/` |
| `rollback` | Undo a recent `init`, `sync`, or `import` from its backup |
| `doctor` | Check the setup and explain what's wrong |
| `customize <tool> [resource]` | Make an editable override for one tool |
| `show <tool>` / `diff [tool]` | Show a tool's final config, or how your overrides differ from the defaults |
| `simplify [tool]` / `resolve` | Clean up overrides that match the defaults, or reconcile them interactively |
| `refresh` | Bring new shipped templates into `.ai/src/` |
| `dedupe` | Remove source files that duplicate a parent project's |
| `skills list\|show\|check` | Inspect the project's skills |
| `mcp list\|show\|validate\|render\|use` | Work with a local catalog of MCP servers |
| `profile add\|list\|remove` | Manage separate config homes, such as work and personal |
| `generate [context]` (`gen`) | Print a prompt that drafts `.ai/src/` for your stack |
| `migrate` | Print a prompt, or run steps, that upgrade an older project |
| `setup-hooks` | Install git hooks that keep generated files current |
| `shell-init [zsh\|bash]` | Print a shell hook that syncs when you `cd` into a project |
| `export` / `import <src>` | Bundle `.ai/src/`, or bring in config and skills from git, an archive, or a folder |
| `update [version]` | Replace the binary with the latest or a given release |
| `upgrade-config` | Move the project's version pin to the installed version |
| `version` (`-v`) / `help` (`-h`) | Print the version or the help |

Every flag, the `--json` output of `sync`, and the less common commands are in the [command reference](docs/guide/commands.md).

## Automation

Generated files go stale when you edit `.ai/src/` or pull someone else's change and forget to sync. Three things can sync for you:

```bash
eval "$(exuno shell-init zsh)"   # in ~/.zshrc: sync when you cd into a project, if anything changed
exuno setup-hooks                # git hooks that suit the project's outputs mode
exuno check                      # in CI: fail when generated files don't match the source
```

[Team setup and automation](docs/guide/team-and-automation.md) explains what each hook does in each mode and how to turn them off.

## Documentation

| Guide | Read it when you want to know |
| --- | --- |
| [How sync works](docs/guide/how-sync-works.md) | What happens during a sync, where each setting comes from, how hand edits are caught |
| [Configuration](docs/guide/configuration.md) | Every `exuno.yaml` option, tool YAML fields, overrides, sources outside the project, adding a tool |
| [Supported tools](docs/guide/tools.md) | What each tool receives, format conversions, models and providers |
| [Skills](docs/guide/skills.md) | Skill categories, inspecting skills, the built-in `exuno` skill, hiding personal skills |
| [Team setup and automation](docs/guide/team-and-automation.md) | Committed or local outputs, version pinning, shell and git hooks, editing generated files |
| [Backups and rollback](docs/guide/backups-and-rollback.md) | What gets backed up, how rollback decides it is safe, retention |
| [Migrating](docs/guide/migrating.md) | Moving existing tool configs in, renaming from AgentSync, upgrading old layouts |
| [Workspaces and profiles](docs/guide/workspaces-and-profiles.md) | Nested projects that share content, separate work and personal config homes |
| [Command reference](docs/guide/commands.md) | Every command and flag |
| [MCP catalog](docs/mcp-library.md) and [skill cards](docs/skill-cards.md) | The read-only catalogs for MCP servers and external skills |

## Why not just...?

- **...symlink the files?** Tools want different extensions (`.mdc`, `.instructions.md`), different frontmatter, and different folder layouts. A symlink can't change the content, and Exuno rewrites it for each tool.
- **...write a shell script per tool?** You'd write the same copy, rename, and header logic once per tool. Exuno is that script, driven by one YAML file per tool and tested on macOS, Linux, and Windows.
- **...stick to the one tool I use today?** Teammates pick different ones, and you may switch later. With one source, adding a tool is `exuno enable <tool>`.

Exuno is one static binary. It doesn't need Node, Python, `yq`, or `jq`.

## Speed

Exuno was a Bash program until 0.37.0 and has been a single Rust binary since 0.38.0. On the repository's benchmark fixture (389 source files, 13 tools enabled, 3465 generated files), on the same machine:

| Command | Bash 0.37.0 | Rust 0.38.0 |
| --- | --- | --- |
| `exuno list` | 0.57 s | under 0.01 s |
| `exuno check` | 73.6 s | 0.25 s |
| `exuno sync` | 67.5 s | 2.5 s |
| `exuno sync --if-stale` | 0.18 s | 0.01 s |

The `check` number matters most. At 74 seconds it couldn't run in a pre-commit hook; at a quarter of a second you don't notice it. The old engine started a new process for every value it read, and the new one reads them in memory. Startup went from 38 ms to 5 ms, and Exuno no longer needs `bash` installed.

The method and the full results are in [`docs/perf/2026-09-19-rust-result.md`](docs/perf/2026-09-19-rust-result.md), compared against [the Bash baseline](docs/perf/2026-09-13-bash-baseline.md) recorded before the rewrite.

## Development

Exuno is a Rust crate at the repository root (edition 2024, `unsafe_code` forbidden). `src/main.rs` is the `exuno` binary, every command lives in `src/cli/`, and the sync engine is in `src/engine/render/`. The tool templates in `lib/templates/` are built into the binary. Configuration is read by `src/config/yaml_subset.rs`, a small parser for the YAML shapes Exuno accepts, so there is no YAML dependency.

```bash
cargo build --release                       # target/release/exuno
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo test                                  # the whole suite
cargo test --test sync                      # one command's tests
```

`cargo test` runs unit tests inside `src/` and integration tests in `tests/*.rs`, one file per command. Each integration test runs the freshly built binary in a throwaway git project; `tests/common/mod.rs` sets that up and removes your `EXUNO_*` and `AGENTSYNC_*` variables and git config so they can't leak in. CI runs the same checks on Linux, macOS, and Windows, plus `shellcheck -x -S warning -e SC1091` on the two remaining shell scripts, `install.sh` and `lib/templates/guard/claude.sh`.

The engine was ported from Bash one command at a time ([design](docs/specs/2026-09-12-rust-migration-design.md)). The Bash version last shipped in 0.37.0 and you can still read it from that tag, for example `git show 0.37.0:lib/helpers/backup.sh`. The module docs in `src/` name the Bash function each file replaced. `scripts/perf/bench.sh` times the binary on a generated 13-tool fixture, and compares it with the Bash engine when `AGENTSYNC_BASH_CLI` points at `bin/agentsync.sh` in a 0.37.0 checkout.

Releases are built by cargo-dist (`dist-workspace.toml`). The auto-tag workflow creates a tag from `VERSION` and starts `release.yml`, which publishes the five archives, their checksums, and the installers.

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
# Exuno itself
rm -rf ~/.exuno && rm -f /usr/local/bin/exuno /usr/local/bin/agentsync
# If a source install added AGENTSYNC_HOME to ~/.zshrc, remove that line

# One project
rm -rf .ai/
# Then delete the AI SYNC GENERATED block from .gitignore
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
