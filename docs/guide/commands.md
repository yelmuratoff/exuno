# Command reference

```
exuno <command> [options]
```

Every command prints its own options and examples with `exuno <command> --help`. This page lists them all and covers the ones that need more than a line.

## All commands

| Command | Alias | What it does |
| --- | --- | --- |
| `init [dir]` | | Set up `.ai/` with starter templates, adopt existing tool config, run the first sync |
| `sync` | | Write every enabled tool's files (`--only`, `--skip`, `--profile`, `--dry-run`, `--force`, `--if-stale`, `--quiet`, `--json`, `--workspace`) |
| `check` | | Exit 0 if outputs match the source, 1 if not |
| `rollback [backup-id]` | | Restore the latest or a chosen backup (`--list`, `--dry-run`, `--force`, `--yes`) |
| `enable <tools…>` | | Turn tools on and copy their settings and hooks into `.ai/src/tools/` for editing |
| `disable <tools…>` | | Turn tools off |
| `list` | `ls` | Show tools and their status |
| `add <kind> <name>` | | Create a `rule`, `skill`, `command`, `subagent`, or `mcp` server |
| `adopt <file>` | | Copy a hand edit in a generated file back into `.ai/src/` (`--all` for every edited file) |
| `customize <tool> [resource]` | | Create an override for one tool |
| `show <tool>` | | Print a tool's final config, after overrides |
| `diff [tool]` | | Show your overrides next to the defaults |
| `simplify [tool]` | | Remove override fields that match the defaults (`--apply`) |
| `resolve` | | Reconcile overrides with the defaults interactively |
| `refresh` | | Bring new shipped templates into `.ai/src/` with a three-way diff (`--status` lists the ones you declined) |
| `dedupe` | | Remove source files that duplicate a parent `.ai/src/`, interactively |
| `migrate` | | Upgrade an older project, or print a prompt that helps an AI do it |
| `profile <cmd>` | | Manage config-home profiles: `add`, `list`, `remove` |
| `doctor` | | Check the setup; report drift, config warnings, and cross-project notes |
| `skills list\|show\|check` | | Inspect the project's skills and their `SKILL.md` metadata (`--profile <name>`) |
| `mcp list\|show\|validate\|render\|use` | | Read a local MCP catalog and prepare a per-tool source, without running any server |
| `generate [context]` | `gen` | Print a prompt that drafts `.ai/src/` for your project |
| `setup-hooks` | | Install the git hooks the project's `outputs:` mode needs (`--pre-commit` in `local` mode) |
| `shell-init [zsh\|bash]` | | Print a shell hook that syncs a project when you `cd` into it |
| `export` | | Bundle `.ai/src/` and the config as `.tar.gz` or `.zip`; `--skill <name>` packages one skill |
| `import <src>` | | Bring in config or skills from a git remote (any ref or folder), an archive, or a folder |
| `update [version]` | | Replace the binary with the latest release, or a given one |
| `upgrade-config` | | Move `exuno_version` in `exuno.yaml` to the installed version |
| `release` | | Bump the version, tag, and push (for maintainers) |
| `version` | `-v` | Print the version |
| `help` | `-h` | Show help |

## `init`

```bash
exuno init                              # wizard in a terminal; detection and defaults elsewhere
exuno init --tools claude,cursor        # these tools
exuno init --yes                        # accept every default
exuno init --dry-run                    # preview
```

| Option | What it does |
| --- | --- |
| `--tools <csv>` | Turn on these tools. Combined with auto-detection unless you pass `--no-detect`. |
| `--content <csv>` | Which source folders to create: `agents`, `rules`, `skills`, `commands`, `subagents` (default: all) |
| `--no-detect` | Don't detect tools from existing folders (`.claude/`, `.cursor/`, `CLAUDE.md`, ...) |
| `--outputs <mode>` | `committed` (default) or `local`; see [Team setup](team-and-automation.md#committed-or-local-outputs) |
| `--existing <action>` | `adopt` (default) copies existing tool config into `.ai/src/`; `replace` regenerates from the shipped templates |
| `--ci github` | Write a GitHub workflow that runs `exuno check`. An existing workflow is kept. |
| `--no-sync` | Skip the first sync |
| `--no-templates` | Create the folders without the shipped starter files |
| `-y`, `--yes` | Skip every prompt |
| `--dry-run` | Show what would be created |

Only the settings, hooks, and MCP files you opt into get copied into `.ai/src/`; the rest come from the shipped defaults at sync time. When `.ai/src/` already exists, `init` skips.

## `sync`

```bash
exuno sync                        # all enabled tools
exuno sync --only claude,cursor   # only these
exuno sync --skip gemini          # all but these
exuno sync --profile hub          # personal tools plus the named profile
exuno sync --dry-run              # preview, write nothing
exuno sync --force                # overwrite edited files and remove hand-added files in generated folders
exuno sync --if-stale             # do nothing unless the source changed since the last sync
exuno sync --workspace            # every .ai/ below the current folder, deepest first
exuno sync --quiet                # only warnings, errors, and the closing [DONE] line
exuno sync --json                 # one JSON summary on stdout after a successful run
```

The log goes to stderr, so `exuno sync --json > out.json` captures only the summary, and `2>&1` merges the log back in. The JSON object is the stable interface for scripts. Its fields are `dry_run`, `synced`, `total`, `skipped`, `written`, `preserved`, and `backup`; new fields may be added, but existing ones won't change. The human-readable log may change between releases.

What a sync does, step by step, is in [How sync works](how-sync-works.md).

## `rollback`

```bash
exuno rollback --list             # list backups
exuno rollback --dry-run          # preview restoring the latest one
exuno rollback <backup-id> --yes  # restore one without asking
```

See [Backups and rollback](backups-and-rollback.md) for when it refuses and how to override that.

## `add`

```bash
exuno add rule testing
exuno add skill deploy
exuno add skill slivers --category flutter/ui
exuno add mcp linear --url https://mcp.linear.app/sse
exuno add mcp github --command npx --args "-y @github/mcp-server"
```

`add mcp` writes to the shared `.ai/src/mcp.json`. It also takes `--env K=V[,K=V...]` for the server's environment. `--force` overwrites an existing file or server entry. Run `exuno sync` afterwards.

## `generate` and `migrate`

```bash
exuno generate                    # print a prompt
exuno generate | pbcopy           # copy it on macOS (xclip on Linux)
exuno generate "React + Next.js"  # with context about your stack
exuno migrate                     # print an upgrade prompt and copy it
```

`generate` is like `claude /init`, except Exuno only prints the prompt. Paste it into any chat AI (Claude, ChatGPT, Gemini), and the AI writes the `.ai/src/` files for your stack and conventions: `AGENTS.md`, rules, skills, commands, agents, and settings.

`migrate` prints a prompt for upgrading an older project; with `--legacy` and `--apply` it runs the migration steps itself. See [Migrating](migrating.md).

## `import` and `export`

```bash
exuno export                                  # ./exuno-bundle.tar.gz
exuno export -o my-config.zip                 # a ZIP
exuno export --skill my-skill                 # ./my-skill.skill, for Claude's skill upload
exuno import https://github.com/user/repo     # also user/repo, git@host:org/repo.git
exuno import https://github.com/user/repo/tree/v2/packages/app
exuno import exuno-bundle.tar.gz --dry-run
exuno import my-skill.skill
```

`import` reads a git remote (`--ref` and `--path` pick the branch and folder; private repositories use your git credentials), an archive (`.tar.gz`, `.tar`, `.tar.xz`, `.tar.bz2`, `.zip`), a skill package, or a folder. `--only rules,skills` limits what it brings in.

An imported skill replaces the project's copy entirely. If the source brings MCP servers, tool config, or skill scripts, `import` lists them under `Runs commands:` and asks before writing them; without a terminal, pass `--force`. It keeps your `exuno.yaml` unless you pass `--config`. An import that changes files is backed up first, so `exuno rollback` undoes it.

## `mcp`

`exuno mcp list`, `show`, `validate`, and `render` read JSON manifests from a local catalog you pick with `--library DIR`:

- `list` prints IDs and titles.
- `show` prints a manifest exactly as stored.
- `validate` checks one entry or the whole catalog.
- `render <id>[@variant]` prints the chosen connection as Exuno MCP source JSON.
- `use <id>[@variant] --tool SLUG` previews a per-tool source. `--apply` writes it, and `--merge --apply` adds one server to an existing per-tool file. Replacing an existing ID needs `--replace <id>`.

None of these start a server or contact an endpoint, and `use` doesn't run `sync` for you. The manifest format and the optional `library.mcp.path` setting are described in the [MCP catalog](../mcp-library.md) page. The opt-in [pilot catalog](../../catalog/mcp/README.md) has entries for Microsoft Learn, Context7, and Octocode, with their requirements and sources.

## `skills`

`exuno skills list`, `show`, and `check` inspect the project's skills; see [Skills](skills.md#inspecting-skills). `exuno skills catalog list` and `show --catalog FILE` read a catalog of external skills you maintain by hand; `--source ALIAS=LOCAL_REPO` reads metadata from a pinned commit in a local git repository. They don't fetch, install, run, or verify anything. See [skill cards](../skill-cards.md) and the [pinned example](../examples/skill-cards/pilot/catalog.tsv).
