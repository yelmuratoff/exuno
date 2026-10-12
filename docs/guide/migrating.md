# Migrating

This page covers four situations: a project that already has tool config, a project from the AgentSync days, a project on an old Exuno layout, and installs of older releases.

## Existing tool configs

If the project already has `.claude/rules/`, `.cursor/rules/`, its own `settings.json`, and so on, run `exuno init`. It adopts that config into `.ai/src/` before the first sync, so nothing is lost. `exuno adopt <file>` does the same for one file at any time.

Don't run a plain `exuno sync` over tool folders you haven't moved in. Sync treats generated folders as its own and replaces or removes anything that isn't in `.ai/src/` (see [What a sync replaces](how-sync-works.md#what-a-sync-replaces)).

When `.ai/src/` already exists, `init` skips, so move files by hand:

1. Move your rules into `.ai/src/rules/`:

   ```bash
   mv .cursor/rules/my-api-conventions.mdc .ai/src/rules/my-api-conventions.md
   # remove the Cursor frontmatter (---/globs/alwaysApply); Exuno adds it back
   mv .claude/rules/testing.md .ai/src/rules/testing.md
   ```

2. Move skills, commands, and agents:

   ```bash
   mv .claude/skills/my-skill/ .ai/src/skills/my-skill/
   mv .claude/commands/deploy.md .ai/src/commands/deploy.md
   mv .claude/agents/security-auditor.md .ai/src/agents/security-auditor.md
   ```

3. Move settings, MCP, and hooks into each tool's folder under `.ai/src/tools/`. MCP servers that every tool shares go in `.ai/src/mcp.json`:

   ```bash
   mkdir -p .ai/src/tools/claude
   mv .claude/settings.json .ai/src/tools/claude/settings.json
   mv .mcp.json .ai/src/mcp.json
   ```

4. Preview, then sync:

   ```bash
   exuno sync --dry-run
   exuno sync
   ```

## Coming from AgentSync

Exuno was called AgentSync up to 0.44. Everything from that time keeps working until 1.0: the `agentsync` command, `AGENTSYNC_*` variables, `.ai/agent_sync.yaml`, `agentsync_version`, `metadata.agentsync-*` skill keys, and git hooks or CI jobs that call `agentsync`.

To move a project to the new names:

```bash
exuno migrate --legacy        # dry run: lists every file, key, and skill still named agentsync
exuno migrate --apply --yes   # renames them, re-pins a pin older than 0.45.0, records format r3
exuno setup-hooks             # rewrites git hooks that still run the agentsync block
```

Migrate only after everyone on the team is on 0.45.0 or newer, because older releases can't read `.ai/exuno.yaml`.

`agentsync update` installs the new release but not the `exuno` command. Rerun the [installer](../../README.md#install) to get it, or run the steps above as `agentsync migrate`.

`install.sh` also links `agentsync` next to `exuno`, so old hooks and CI keep working until 1.0. The Windows PowerShell installer adds only `exuno`; in a project whose git hooks predate the rename, run `exuno setup-hooks` once. `exuno doctor` lists any hook that still calls `agentsync`, along with anything else left to rename.

An earlier install in `~/.agentsync` is no longer used once you rerun the installer, which tells you how to remove it.

## From the 0.10 flat layout

Projects upgraded from 0.10 keep working, because Exuno still reads `.ai/src/{hooks,mcp,settings}/<tool>.<ext>`. When you're ready to move to the per-tool layout:

```bash
exuno migrate --legacy    # dry run: prints the planned moves
exuno migrate --apply     # moves the files and merges identical MCP files
exuno migrate --apply -y  # same, without prompts; accepts the MCP merge
```

Each legacy file moves to `.ai/src/tools/<tool>/<resource>.<ext>`. If every `.ai/src/mcp/*.json` is identical, `migrate` offers to collapse them into one shared `.ai/src/mcp.json`; if they differ, each moves to its tool's folder. A file that already exists at the target is never overwritten; `migrate` skips it with a warning so you can resolve it by hand. Empty source folders are removed afterwards.

`migrate` also finds the pre-0.6 `.agent/` folder (singular), which held `AGENTS.md`, `workflows/`, `rules/`, and `skills/` with no per-tool split. The current engine doesn't recognize it, so a normal `sync` never cleans it up. The dry run lists what's inside, and `--apply --yes` removes it. One `migrate --apply --yes` handles both the flat layout and `.agent/`.

## Upgrading with an AI assistant

`exuno migrate` with no flags prints a prompt for a coding AI that can read your repository, and copies it to the clipboard with `pbcopy`, `wl-copy`, `xclip`, `xsel`, or `clip.exe`, whichever exists. Without a clipboard tool, it's still printed.

The prompt includes the installed version and the project's `exuno_version` pin. It tells the AI to confirm the real starting version, read every relevant changelog section, check the README, the bundled skill, and the templates from the same stable release, make a checkpoint it can return to, keep your custom config, use the supported migration commands, and finish with `doctor`, `sync`, and `check`.

## The format revision

`format:` in `.ai/exuno.yaml` records which migrations a project has been through. It's a small counter that goes up only when a project actually needs a step, unlike `exuno_version`, which changes with every patch. That way you get a reminder only when there's something to do. `init` writes the current revision, `migrate --apply` records it, and nothing else touches it. A project behind the engine is flagged on the next interactive command and by `exuno doctor`.

## Older releases and source installs

`exuno update <version>` and `EXUNO_VERSION` can install any release. Releases before 0.45.0 ship as `agentsync-<target>` archives, and both the installer and `update` still fetch those.

Releases from before the first binary release have no archive. Pinning to one installs from source: a git clone in `~/.exuno/`, with `AGENTSYNC_HOME` added to your shell config, which is what the installer always did for those versions. Such an install switches to the binary on its own the next time `exuno update` reaches a release that ships one.
