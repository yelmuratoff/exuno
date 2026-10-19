# Configuration

Exuno reads two kinds of config. The project config, `.ai/exuno.yaml`, says which tools are on and how the project behaves. A tool YAML says where one tool's files go and in what format. Exuno ships a tool YAML for every supported tool, and you override one only when you need something different.

## The project config: `.ai/exuno.yaml`

`init` writes this file. Projects from the AgentSync days may still have `.ai/agent_sync.yaml` or `agent_sync.yaml` in the root, which keep working until 1.0. To use a file somewhere else, set `EXUNO_CONFIG_PATH`; every command that reads the config then fails if that file is missing, without falling back to `.ai/exuno.yaml`.

Every key is optional. The ones you are most likely to touch:

```yaml
exuno_version: "0.46.0"   # the pinned engine version, written by init
outputs: committed        # or local: are generated files committed?

tools:
  enabled:
    - claude
    - cursor
```

| Key | What it controls | Where it's explained |
| --- | --- | --- |
| `tools.enabled` | Which tools sync. `exuno enable` and `disable` edit it. | [Supported tools](tools.md) |
| `exuno_version` | The engine version every machine must run | [Team setup](team-and-automation.md#pin-the-exuno-version) |
| `version_pin` | Whether a version mismatch is fatal in `local` mode | [Team setup](team-and-automation.md#pin-the-exuno-version) |
| `outputs` | `committed` or `local` | [Team setup](team-and-automation.md#committed-or-local-outputs) |
| `source.*` | Where the sources live | [Source paths](#source-paths) below |
| `shared` | Inherit content from a parent project | [Workspaces](workspaces-and-profiles.md#workspaces-nested-projects) |
| `profiles` | Extra config homes, such as work and personal | [Profiles](workspaces-and-profiles.md#profiles-separate-config-homes) |
| `skills.hide` | Personal skills this project replaces | [Skills](skills.md#hiding-a-personal-skill) |
| `base_skills` | `false` drops the built-in `exuno` skill | [Skills](skills.md#the-built-in-exuno-skill) |
| `backup.retention` | `bounded` (default) or `preserve` | [Backups](backups-and-rollback.md#how-long-backups-are-kept) |
| `post_sync.skip` | Turn off a tool's `post_sync` command | [`post_sync`](#post_sync) below |
| `gitignore.update` | `false` stops Exuno from editing `.gitignore` | [Team setup](team-and-automation.md#committed-or-local-outputs) |
| `library.mcp.path` | A default MCP catalog for `exuno mcp` | [MCP catalog](../mcp-library.md) |
| `format` | Which migrations the project has been through | [Migrating](migrating.md#the-format-revision) |

Quote any value that contains `#`. Exuno reads an unquoted value up to the first `#`, with or without a space before it, so `url: https://x.dev/#top` reads as `https://x.dev/`, while `url: "https://x.dev/#top"` keeps the whole URL. This applies to tool YAML files too.

## Source paths

By default the sources are under `.ai/src/`. To put them somewhere else, set `source.*`. Relative values are resolved from the project root, including when `EXUNO_CONFIG_PATH` points elsewhere and inside `check`'s temporary workspace.

```yaml
source:
  agents: ".ai/src/AGENTS.md"
  rules: ".ai/src/rules"
  skills: ".ai/src/skills"
  commands: ".ai/src/commands"
  subagents: ".ai/src/agents"
  tools: ".ai/src/tools"
```

`source.tools` covers both the tool YAML files and each tool's settings, hooks, and MCP folder (`<source.tools>/<tool>/settings.json`, `hooks.json`, `mcp.json`), so they always stay together.

### Sources outside the project

You can keep one source tree for several projects, for example in your home folder. Point the sources there and keep the outputs in the project:

```yaml
source:
  agents: "/home/me/agentic/.ai/AGENTS.md"
  rules: "/home/me/agentic/.ai/rules"
  skills: "/home/me/agentic/.ai/skills"
  tools: "/home/me/agentic/.ai/tools"
```

Then trust that folder from outside the repository, in your shell profile or CI environment:

```bash
export EXUNO_EXTERNAL_SOURCE_ROOTS="/home/me/agentic"   # colon-separated
```

Only the environment can grant that trust, never `exuno.yaml`. That way, syncing a repository you cloned (including the `shell-init` hook when you `cd` into it) can't read your files from wherever its config points. The rules:

- **The folder must be trusted.** A source outside the project must resolve under an absolute folder listed in `EXUNO_EXTERNAL_SOURCE_ROOTS`. Otherwise `sync` and `check` stop before writing, and `doctor` reports it.
- **Only explicit `source.*` values can point outside.** A `source.agents`, `source.rules`, `source.skills`, `source.commands`, `source.subagents`, or `source.tools` value in the selected project config may be an absolute path or a `../` path. Nothing else can widen where sources are read from: the defaults, the auto-detected `.ai/src/` and `.ai/` layouts, and values that stay inside the project all keep to the project.
- **Symlinks follow the same rule.** Before reading anything, `sync` and `check` resolve every symlink under `.ai/` and the configured sources, following chains and links to folders. A link that leads outside the project and outside the trusted folders stops the run and is named. A committed `.ai/src/rules/notes.md -> ~/secrets.md` is the case this blocks. A link to a shared tree you maintain works once that tree is trusted.
- **Some folders are always refused.** A value that resolves to `/`, your home folder, the project root, or a folder that contains the project root is rejected before anything is written. `doctor` reports it for every key except `tools`.
- **Outside sources are read-only.** Outputs stay inside the project, and `check` reads the sources in place while writing only to its temporary workspace.
- **Nothing writes into an outside tool catalog.** When `source.tools` is outside the project, `customize`, `profile add` and `remove`, `adopt`, `simplify --apply`, `enable --scaffold`, and a `disable` that would flip a legacy `enabled: true` all exit with an error before writing. Plain `enable` skips scaffolding. Edit that catalog where it lives.

## Changing one tool: overrides

Three commands cover most changes:

- `exuno enable <tool>` turns the tool on, copies its settings and hooks into `.ai/src/tools/<tool>/` so you can edit them, and prints the path to edit. `--no-scaffold` skips the copies; `--yes` answers the confirmation without a terminal.
- `exuno add mcp <server> [--command … | --url …]` adds a server to the shared `.ai/src/mcp.json`, which every enabled tool with an MCP target receives.
- `exuno customize <tool>` creates an empty tool YAML override, `.ai/src/tools/<tool>.yaml`, where you write only the fields you want to change; `--full` copies the whole base instead. `exuno customize <tool> <resource>` makes an editable copy of the `hooks`, `mcp`, or `settings` file, for example `exuno customize cursor hooks`. Use it when one tool needs something different from the shared source, or to get a file that `enable --no-scaffold` skipped. `exuno customize <tool> mcp` works when that tool ships an MCP base.

All three write to `.ai/src/tools/<tool>/` or `.ai/src/mcp.json`. The older folders `.ai/src/hooks/`, `.ai/src/mcp/`, and `.ai/src/settings/` are still read; see [Migrating](migrating.md#from-the-010-flat-layout).

To see the result, `exuno show <tool>` prints a tool's final config and `exuno diff [tool]` shows where your overrides differ from the base. `exuno simplify` removes override fields that match the base, and `exuno resolve` walks through them interactively.

## Tool YAML

Each tool is one YAML file. The shipped ones are built into the binary; a file in `.ai/src/tools/` with the same name overrides it field by field. The file name is the tool's ID for `--only` and `--skip`. Files whose names start with `_` are ignored.

The shipped one for Claude Code, slightly shortened:

```yaml
name: "Claude Code"

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

The `source:` paths under `settings` and `mcp` are the old layout. They are kept so older projects still work, and the files in `.ai/src/tools/claude/` win over them automatically, so you don't need to set them. Which tools sync is decided by `tools.enabled` in `exuno.yaml`, not by the tool YAML.

The full shape, with every optional field commented out:

```yaml
name: "Tool Name"

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

# post_sync: "npx prettier --write .tool/**/*.mdc"
```

### Fields

| Field | What it does |
| --- | --- |
| `extension` | Renames the file extension (`.mdc`, `.instructions.md`, `.agent.md`, `.prompt.md`) |
| `header` | Text added to the top of each file, usually YAML frontmatter (Cursor, Windsurf, Copilot) |
| `scoped_header` (rules) | Used instead of `header` for a rule with `paths:` frontmatter. `{globs}` becomes its globs joined by commas, `{globs_list}` a quoted YAML list (Cursor, Copilot, Windsurf, Antigravity, Kiro) |
| `include` / `exclude` | Glob filters on file names. For skills they also match a category path, so `exclude: cloudflare/*` drops a whole category. |
| `append_imports` | Adds `@rules/*` import lines to the agents file (Claude) |
| `merge_to_file` | Merges all rules into one file (Zed) |
| `prepend_agents` | Puts the `AGENTS.md` content before the merged rules (Zed) |
| `inline_into_agents` (rules) | Adds a short reference to each rule (name and title) to the agents file (Codex, Gemini, Junie, Kimi Code, MiniMax Code, OpenCode) |
| `inline_into_agents` (skills) | Adds a skill index (name and description) to the agents file (Amazon Q, Zed) |
| `inline_into_agents` (commands) | Adds a `## Commands` index (`` `/<name>` — description ``) to the agents file (Amazon Q, Zed) |
| `as_skills` (commands) | Writes each command as a generated skill at `<skills dest>/command-<name>/SKILL.md` (Codex, Kimi Code, Kiro) |
| `format` | Converts the file. `toml` for Gemini commands and Codex subagents, `amazonq_json` for Amazon Q agents, `opencode_md` and `kiro_md` for OpenCode and Kiro agents, `opencode_json` and `codex_toml` for MCP |
| `source` (settings, MCP, hooks) | A declared source path. The canonical `.ai/src/tools/<tool>/<resource>.<ext>` beats it automatically. |
| `enabled` (on a target) | `false` turns off one output category while the rest keep syncing |
| `legacy_dest` | Where an earlier config wrote the target. Once the target moves, sync removes the files there that the manifest recorded (Windsurf, Cline) |
| `guard` | Copies the tool's write-guard script to `dest` and makes it executable. Override it at `.ai/src/tools/<tool>/guard.sh` and register it from the tool's settings (Claude). |
| `profile_scoped: false` | On any target: `exuno profile add` keeps the base `dest` instead of moving it into the profile's config home |
| `profile_supported: false` | Refuses config-home profiles for a tool that reads only project files |
| `adoptable: false` (agents) | Refuses `adopt` when the agents file can't be copied back into its source safely |
| `base` | Makes this tool a variant of another one that inherits everything else (used by profiles) |

[`lib/templates/tools/_TEMPLATE.yaml`](../../lib/templates/tools/_TEMPLATE.yaml) documents every option, including ones this table leaves out.

### `post_sync`

`post_sync` runs any shell command after a tool syncs, so Exuno skips it with a warning unless `EXUNO_ALLOW_POST_SYNC=true` is set in the environment. The `exuno.yaml` in the repository can't grant this, so cloning and syncing an untrusted repository never runs its command. `EXUNO_SKIP_POST_SYNC=true` or `post_sync.skip: true` in `exuno.yaml` turns it off again, and `check` always skips it.

A `post_sync` command can change anything, and changes it makes outside the tool's targets can't be undone by [rollback](backups-and-rollback.md).

## Turning off a tool or a category

To manage a tool by hand, take it out of `tools.enabled` with `exuno disable <tool>`. A legacy tool YAML may say `enabled: true` itself; set it to `false`:

```yaml
# .ai/src/tools/cursor.yaml
enabled: false
```

To skip a tool for one run, use `exuno sync --skip cursor`.

To keep a tool on but drop one kind of output, set that target's `enabled` to `false`. Everything else keeps syncing:

```yaml
# .ai/src/tools/claude-hub.yaml
base: claude
targets:
  rules:
    enabled: false
```

This works for every entry under `targets:`. It's handy when a `base:` variant inherits a destination that another tool should own.

## Using `AGENTS.md` for Claude Code too

Exuno already writes the root `AGENTS.md` as the instructions file for Codex, Cursor, OpenCode, Windsurf, Cline, Kiro, and MiniMax Code. Claude Code reads `AGENTS.md` in a folder that has no `CLAUDE.md`, so a project that wants one instructions file can turn Claude's off:

```yaml
# .ai/src/tools/claude.yaml
targets:
  agents:
    enabled: false
```

```bash
rm CLAUDE.md          # only if an earlier sync wrote it
exuno sync
```

Claude Code keeps everything else: `.claude/rules/`, skills, commands, subagents, settings, hooks, and MCP are separate targets.

Delete the old file as well. A `CLAUDE.md` left from an earlier sync is still read by Claude Code, and since nothing updates it, it slowly goes out of date. Sync leaves it in place in case you kept it on purpose; `exuno check` and `exuno doctor` mention it without failing until it is edited, which makes sync refuse it and `check` fail. Once it's gone, the next sync drops it from the manifest.

## Adding a new tool

A tool Exuno doesn't ship is a YAML file in your project. Start from the closest shipped tool instead of an empty file:

```bash
exuno customize cursor --full        # copies the whole base into .ai/src/tools/cursor.yaml
cp .ai/src/tools/cursor.yaml .ai/src/tools/newtool.yaml
# edit newtool.yaml: at least name: and one target's dest:
exuno enable newtool
exuno sync --only newtool
```

Every field is documented in [`lib/templates/tools/_TEMPLATE.yaml`](../../lib/templates/tools/_TEMPLATE.yaml). A file whose name starts with `_` is never read as a tool, so you can keep a reference copy around.
