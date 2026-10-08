# Maintenance

Read this when running `exuno update`, `exuno resolve`, `exuno simplify`, `check`, `doctor`, `rollback`, `migrate`, or `upgrade-config`, or when a teammate reports stale-override or upstream-drift problems.

## Resolving upstream drift

When you run `exuno update`, the CLI compares the tool catalog built into the running binary against the catalog built into the downloaded one, field-by-field. For every upstream change to a field you have overridden, the update prints a warning and writes the list to `.ai/.pending-resolutions.yaml`:

```yaml
schema: 1
from_version: "0.7.0"
to_version: "0.8.0"
conflicts:
  - tool: "claude"
    field: "targets.rules.dest"
    base_before: ".claude/rules"
    base_after: ".claude/rules-v2"
    your_override: ".claude/my-rules"
```

- `exuno resolve` reads this queue on startup and flags the affected fields with `⚡`. Walking through every override (not a subset with `resolve <tool>`) clears the queue automatically.
- Pass `--strict` to `exuno update` to exit non-zero on any conflict — useful in CI to block a merge until someone reviews upstream changes.
- The file is non-authoritative: delete it any time if you prefer to ignore the warnings. Your overrides are untouched until you explicitly adopt a base value via `exuno resolve`.

## Simplifying redundant overrides

After `exuno customize <tool> --full`, an override carries the entire base template verbatim. Over time those redundant fields pin stale values and silently block upstream updates — if base moves forward, the redundant override wins and you stay on the old value.

`exuno simplify` walks every user override and drops fields that already match the current base, leaving only the ones that actually diverge.

```
exuno simplify              # dry-run every override
exuno simplify cursor       # dry-run just one tool
exuno simplify --apply      # persist changes
exuno simplify --apply -y   # persist + auto-delete emptied files
```

- Dry-run by default — prints a preview of fields that would be removed and fields that would stay. Pass `--apply` to write.
- If every overridden field matches base, the entire override file is redundant. With `--apply -y` the file is deleted automatically; in an interactive shell without `-y` you're prompted.
- Idempotent: running with `--apply` twice in a row is a no-op the second time.
- Comments inside a user override are not preserved when a nearby field is removed — the line-level YAML mutator strips the key and any indented comments below it. If you rely on inline documentation, keep a separate note or use `exuno show <tool>` to re-derive intent.

## Backups and rollback

`exuno init` and `sync` snapshot every path they may mutate under the Git-ignored `.ai/backups/` and automatically restore it on failure. Use `exuno rollback --list`, preview with `rollback [<id>] --dry-run`, and restore the latest or selected snapshot with `rollback [<id>]`; pass `--yes` outside a TTY. Rollback creates a safety snapshot first, so it can itself be undone. It refuses, naming the first changed path, when a target changed after the snapshot's operation finished; roll back newer snapshots first or pass `--force` to restore anyway.

- Backups cover declared tool destinations and Exuno's manifest and `.gitignore` state. A trusted `post_sync` hook can mutate arbitrary paths; side effects outside declared destinations are not automatically reversible.
- After an operation, history is pruned to snapshots that are both among the latest 10 (`EXUNO_BACKUP_LIMIT`) and younger than 30 days (`EXUNO_BACKUP_MAX_AGE_DAYS`); either set to `0` disables that bound, and the newest snapshot is always retained. Nest `retention: preserve` under a `backup:` key in `exuno.yaml` to keep every existing snapshot and staging entry instead; `bounded` is the default, and any other value stops init, sync, and rollback before they write.

## Other commands

- `exuno list` shows configured tools and status; `exuno enable` / `disable <tool>` toggle them.
- `exuno check` verifies generated output matches source and exits non-zero on drift (use it in CI).
- `exuno doctor` validates the setup and surfaces drift, config warnings, and cross-project advisories.
- **Keep outputs fresh automatically:** add `eval "$(exuno shell-init zsh)"` to `~/.zshrc` to auto-sync when the current directory is an Exuno project root, without syncing parent projects from nested directories (silent no-op when nothing changed; eval'ing keeps it current across upgrades); `exuno setup-hooks [--pre-commit]` syncs on `git pull` / `checkout`; `exuno sync --if-stale` syncs only when source changed since the last sync. See the README "Automation" section.
- `exuno generate [context]` prints a prompt you paste into any AI to draft a project-specific `.ai/src/`.
- `exuno export` bundles `.ai/src/` into an archive, and `exuno export --skill <name>` packages one skill as a `.skill` file for Claude's skill upload; `exuno import <src>` pulls a config from a repo, archive, or directory. A `.skill` package, or a folder or `.zip`/`.tar.gz` of skill folders, lands in `.ai/src/skills/`, where the project already keeps a skill of that name.
- `exuno migrate` prints and copies a grounded prompt for safely upgrading an existing project to the latest documented Exuno format. Use `exuno migrate --legacy` to preview legacy flat-layout moves and engine-owned skill copies, and `exuno migrate --apply` to perform them.
- `exuno upgrade-config` re-pins the engine version in `exuno.yaml`.
- `version_pin.mode` controls pin mismatches in `exuno.yaml`: `committed` outputs remain strict, while `local` outputs warn by default. Nest `mode: strict` under a `version_pin:` key to make local mismatches fatal; `warn` preserves the default. A top-level `version_pin.mode:` line is not read. The scalar shorthand `version_pin: strict` (or `warn`) is also accepted. Unknown modes are rejected before writing.
- `outputs:` in `exuno.yaml` picks where generated files live: `committed` (the `init` default) keeps outputs and `.ai/.sync-manifest` in git so teammates need only `git pull` and CI runs `exuno check`; `local` gitignores both and every clone runs `exuno sync`. The manifest always shares the outputs' git status. In `committed` mode `sync` and `check` refuse to run when `exuno_version` differs from the engine — match it with `exuno update <version>` or move it with `exuno upgrade-config`.

## Recommended cadence

- **After each `exuno update`:** review `.ai/.pending-resolutions.yaml` (if present) and run `exuno resolve` before continuing other work.
- **Quarterly or after a major version bump:** run `exuno simplify` (dry-run first) to drop stale fields; commit the result on its own so the diff is reviewable.
- **In CI for a config-stable repo:** add `exuno update --strict` so unreviewed upstream drift fails the build.
