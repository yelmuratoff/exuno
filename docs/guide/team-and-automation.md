# Team setup and automation

Generated files come from `.ai/src/`, so they go stale whenever someone edits the source and doesn't sync. This page covers how a team keeps them current: whether to commit them, how to pin the Exuno version, and which hooks sync for you.

## Committed or local outputs

`outputs:` in `.ai/exuno.yaml` decides whether generated files go into git. `exuno init` writes `committed`; `exuno init --outputs local` picks the other mode.

| Mode | What's in git | Who runs Exuno |
| --- | --- | --- |
| `committed` (default) | `.ai/src/`, the generated files, `.ai/.sync-manifest` | Whoever edits `.ai/src/` runs `sync`; CI runs `check` |
| `local` | `.ai/src/` only | Every clone, after every pull (`setup-hooks` does it) |

With `committed`, teammates don't need Exuno: `git pull` brings the generated files already up to date. Whoever changes the rules runs `exuno sync` and commits the source and the outputs together.

With `local`, everyone installs Exuno and runs `exuno setup-hooks` once, so a sync follows every pull and checkout.

In both modes `exuno sync` manages a block in `.gitignore` between `AI SYNC GENERATED START` and `END`. In `local` mode it lists every generated path and the manifest. In `committed` mode it lists only profile config homes, which are personal either way. The manifest always has the same git status as the files it describes; otherwise a teammate's `git pull` would look like a hand edit on the next sync.

A project without an `outputs:` key behaves as `local`, or as `committed` if it already set `gitignore.update: false`.

## Pin the Exuno version

Different Exuno versions can write slightly different files. With committed outputs, every machine and CI must produce identical bytes, so `init` writes `exuno_version` into `.ai/exuno.yaml`, and `sync` and `check` stop when the running version doesn't match.

To match the pin, install that version:

```bash
exuno update 0.45.0
# or, on a fresh machine or in CI
EXUNO_VERSION=0.45.0 curl -fsSL https://raw.githubusercontent.com/yelmuratoff/exuno/main/install.sh | bash
```

To move the pin to the version you have, run `exuno upgrade-config` and commit the re-synced outputs.

In `local` mode a mismatch is only a warning. `version_pin` makes it fatal there too:

```yaml
version_pin:
  mode: strict # or warn (the default)

# or the short form:
version_pin: strict
```

An unknown mode is rejected before a sync can write anything.

## Keeping agents on the source

An agent that edits a generated file loses the change on the next sync. Three things keep agents on `.ai/src/`:

- The shipped `AGENTS.md` and `rules/core.md` say where instructions live.
- Claude Code gets a `PreToolUse` hook, `.claude/hooks/agentsync-guard.sh`, that blocks a write to any file listed in `.ai/.sync-manifest` and names its source instead. Replace it per project at `.ai/src/tools/claude/guard.sh`, or remove the `hooks` block from your settings override to drop it.
- `sync` refuses to overwrite a generated file that was edited since the last run.

## Editing a generated file

If you edited a generated file directly, for example while working in Claude Code or Cursor, `adopt` moves the edit into `.ai/src/`, so you don't have to copy it over and run `sync --force`:

```bash
exuno adopt .claude/rules/core.md            # interactive, shows a diff first
exuno adopt --dry-run .claude/rules/core.md  # show the plan, write nothing
exuno adopt --yes .claude/rules/core.md      # no prompt (CI, scripts)
exuno adopt --all                            # every edited file at once
```

`adopt` finds the source behind the file (`.ai/src/rules/core.md` here), copies the edit into it, and updates the manifest, so the next `sync` sees no drift.

`--all` adopts every edited file listed in the manifest in one pass and lists the ones it had to skip. If two edited files lead back to the same source with different content (for example `CLAUDE.md` and `GEMINI.md` both come from `.ai/src/AGENTS.md`), both are skipped so neither overwrites the other. Adopt one of them explicitly.

Some files can't be adopted, because copying them back would damage the source:

- Rules that get a frontmatter header (`cursor`, `copilot`, `windsurf`, `antigravity`). Adopting would push that tool's header into every other tool's rules.
- Rules merged into one file (`zed`). One file can't be split back into many.
- Rules or skills listed in an agents file (rules: `codex`, `gemini`, `junie`, `kimi`, `minimax`, `opencode`; skills: `amazonq`, `zed`).
- Converted files (`codex` subagents in TOML, `amazonq` subagents in JSON, `opencode` subagents in OpenCode Markdown).

For those, edit `.ai/src/` directly. Exuno names the file when it refuses.

If you'd rather throw the edit away, `exuno sync --force` overwrites edited files and removes files you added by hand to generated folders.

## Syncing automatically

### Shell hook

Add this line to `~/.zshrc` (or `~/.bashrc`, with `bash`):

```bash
eval "$(exuno shell-init zsh)"
```

When you `cd` into a folder that is an Exuno project root, or open a shell there, the hook runs `exuno sync --if-stale`. If nothing changed, it does nothing and prints nothing. It doesn't look for a parent project while you move around its subfolders. Set `EXUNO_NO_AUTO_SYNC=1` to turn it off without removing the line.

The `eval` form regenerates the hook from `exuno` in every new shell, so an upgrade reaches it without touching your rc file. `direnv`, `starship`, and `zoxide` work the same way. If you'd rather not call `exuno` in every new shell, save a copy with `exuno shell-init zsh >> ~/.zshrc` and repeat that after each upgrade.

### Git hooks

```bash
exuno setup-hooks                 # the hooks this project's outputs mode needs
exuno setup-hooks --pre-commit    # local mode: also sync before each commit
```

What `setup-hooks` installs depends on `outputs:`:

- **`committed`**: one pre-commit hook. It runs `sync --if-stale` and fails the commit when a generated file would be left out of it, so outputs never fall behind the source.
- **`local`**: `post-merge` and `post-checkout` hooks that run `exuno sync` after `git pull` and `git checkout`. `--pre-commit` adds a hook that runs `sync --if-stale` before each commit. None of these block git: if the sync fails, you get a warning and git goes ahead.

`EXUNO_SKIP_HOOKS=1` turns every installed hook into a no-op. You can run `setup-hooks` again at any time: a current Exuno block is left alone, a block from an older release is rewritten in place, and the rest of each hook file is kept.

### CI and scripts

```bash
exuno sync --if-stale    # sync only if the source changed since the last sync
exuno check              # exit 0 if outputs match the source, 1 if not
```

`--if-stale` is the cheap check the shell and git hooks use. `check` is the authoritative one, and it belongs in CI. When the project has a `.github/` folder, `exuno init` offers to add a workflow that runs it (`--ci github` adds it without asking). With committed outputs, install the pinned version in CI with `EXUNO_VERSION`.
