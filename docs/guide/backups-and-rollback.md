# Backups and rollback

Every command that changes your files backs them up first. If it fails, Exuno puts everything back. If it succeeds but you didn't want it, `exuno rollback` undoes it.

## What gets backed up

Before a real `init`, `sync`, `import`, or `mcp use --apply` writes anything, Exuno copies every path it may change into `.ai/backups/<backup-id>/`. That covers the tools' destinations, `.ai/.sync-manifest`, and the managed part of `.gitignore`. The backup folder ignores itself in git.

No backup is made for a dry run, a `sync --if-stale` that finds nothing to do, or a sync that stopped because of hand edits.

Only one run can change a project at a time. A run takes a project lock before its backup, and a second run started meanwhile is refused. If a run is killed partway through, the next run finds its unfinished record and restores the backup before doing anything else.

A trusted `post_sync` command can run anything. Changes it makes outside the tool's targets aren't covered by the backup.

## Undoing a run

```bash
exuno rollback --list                 # list complete backups
exuno rollback --dry-run              # preview restoring the latest one
exuno rollback                        # restore the latest (asks first)
exuno rollback <backup-id> --yes      # restore a specific one without asking
```

Rollback makes its own backup first and prints its ID, so you can undo the rollback too.

### When rollback refuses

Rollback won't quietly overwrite work done after the run you're undoing. Before restoring anything, it compares every target with the state recorded right after that run. Any later change stops the whole rollback and names the first path that differs. That includes added, edited, or deleted files, a changed type or executable bit, and even a stray file inside a folder, like a `.DS_Store`. A file recorded as absent isn't treated as permission to delete a file someone created later.

- `--yes` only skips the question. It doesn't skip this check.
- `--dry-run` shows the plan and the conflict, and exits 1.
- `--force` skips the check and restores anyway, still making the safety backup.

To go back further than the latest run, undo the newer runs first, newest to oldest, or use `--force` to jump straight to the older state.

### How the check works

When `init`, `sync`, or `rollback` finishes, it writes an `after.tsv` record into its backup: one line per file, folder, symlink, or missing target, with a content hash and the executable bit. When you undo a rollback, the expected state is the result of that rollback.

If the record can't be written, because there's no `sha256sum` or `shasum` or a file can't be read, the run still succeeds with a warning. A backup without a usable record (one older than this check, one left unsealed by such a warning, or one with an empty or damaged `after.tsv`) is restored as before, with a warning that later changes can't be detected.

Symlinks are compared by the path they point to. Their targets aren't inspected or restored, so a change behind an unchanged link neither blocks rollback nor gets reverted. Replacing the link itself is a conflict. A target reached through a symlinked parent folder is compared through it while the link stays inside the project; one that leads outside is refused.

The check runs once before the plan and again after the safety backup, right before restoring. It isn't atomic: another program can still change files between the last check and a write. Stop other writers while you sync or roll back. The recovery that happens inside a failing run is separate from the rollback you ask for, and the record doesn't protect against someone who can rewrite the backup folder.

## How long backups are kept

After each operation, successful or restored after a failure, Exuno prunes old backups. A backup is kept if it is among the latest 10 and younger than 30 days. The newest one is always kept, so rollback still works after a project sits idle for months.

- `EXUNO_BACKUP_LIMIT` changes the count and `EXUNO_BACKUP_MAX_AGE_DAYS` changes the age. Each takes a non-negative integer; `0` turns that limit off.
- Temporary folders left by an interrupted run are removed on the next backup once they're more than 24 hours old.

To keep everything, set this in `.ai/exuno.yaml` (or the root `exuno.yaml`, or the file `EXUNO_CONFIG_PATH` names):

```yaml
backup:
  retention: preserve
```

`backup.retention` is `bounded` (the default described above) or `preserve`. With `preserve`, Exuno prunes no backups and leaves existing `.tmp.*`, `.latest.tmp.*`, and `.gitignore.tmp.*` staging entries alone, whatever the count and age limits say. Setting both environment limits to `0` doesn't do this on its own; it still cleans staging entries. With `preserve` the folder can grow without limit, so remove old backups yourself when you no longer need them.

A few details:

- `init`, `sync`, and `rollback` validate the policy and limits before they touch anything. `rollback --list` and `check` change nothing and skip that validation.
- An empty or unknown `retention` value is an error.
- The policy is fixed for the whole operation, including the automatic recovery after a failed sync and a rollback that restores a different config.
- New backups are still made, and the `.latest` pointer and `.gitignore` metadata are still updated. Temporary files the current run created can still be cleaned up if it fails; `preserve` protects recovery data that existed before the run started.
