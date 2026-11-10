# Rust Engine Migration

Date: 2026-09-12
Status: Complete on 2026-09-19, started 2026-09-12. Phases 1 and 2 are closed in
`docs/plans/2026-09-12-rust-migration-phase-1-native-list.md` and
`docs/plans/2026-09-13-rust-migration-phase-2-native-check.md`; Phase 3 is
closed in `docs/plans/2026-09-14-rust-migration-phase-3-native-sync.md`.
Phase 3b, release 0.36.0's Bash changes, is closed in four family plans ending
with `docs/plans/2026-09-14-rust-migration-phase-3b-rollback-witness.md`.
Phase 4 is closed in thirteen command-family plans, the last being
`docs/plans/2026-09-16-rust-migration-phase-4m-tail.md`. Phase 5 is closed in
five slice plans, from `docs/plans/2026-09-16-rust-migration-phase-5a-entry.md`
to `docs/plans/2026-09-18-rust-migration-phase-5e-cutover.md`, on 2026-09-18;
0.37.0 was the cutover release, the first to ship the binary and the last to
carry the Bash engine. Phase 6 is closed on 2026-09-19 in
`docs/plans/2026-09-19-rust-migration-phase-6-retire-bash.md`: the Bash engine
is deleted and the suite runs against the binary on every platform. Phase 7 is
closed in `docs/plans/2026-09-19-rust-migration-phase-7a-harness-cli-list-check.md`
and `docs/plans/2026-09-19-rust-migration-phase-7b-port-the-suite.md`: the bats
suite is ported to Rust integration tests, one commit per file, and bats is
retired. The migration is complete.

## Objective

Replace the Bash engine with one statically linked Rust binary, command by
command, so that a user never sees a behaviour change until the cutover release
and never needs Git Bash on Windows after it.

The result must preserve AgentSync's existing properties:

- `.ai/src/` remains the source of truth.
- Generated outputs are byte-identical to what 0.35.x writes for the same
  source, config, and enabled tools.
- Drift protection, the transactional backup and rollback, the
  `agentsync_version` pin, and the committed/local outputs modes keep their
  semantics and their on-disk formats.
- The guard hook stays plain POSIX `sh`: a teammate without the CLI must still
  be protected.
- The 725-test bats suite stayed the behavioural contract until Phase 7
  rewrote it as Rust integration tests, case by case.

## Why

Measured on a 392-file, 98-skill source, engine at 0.35.2, macOS. These numbers
motivated the migration; they are not the baseline its result is judged against,
because the fixture was not pinned and no method was recorded.
`docs/perf/2026-09-13-bash-baseline.md` holds that baseline, on a generated
fixture, with the discrepancies between the two written down, and
`docs/perf/2026-09-19-rust-result.md` holds the result measured against it:

| Command | Wall time | user / sys |
| --- | --- | --- |
| `sync`, 13 tools, 272 outputs | 6.2–6.7 s | 2.8 s / 3.5 s |
| `check`, 13 tools | 7.0 s | 2.9 s / 4.0 s |
| `sync`, 2 tools | 2.0 s | 1.0 s / 0.9 s |
| `sync --if-stale`, no-op | 0.15 s | |

System time exceeds user time: the engine is fork-bound. 0.35.1 removed
subshells from the hot paths and gained 23%; the remaining ~1,600 command
substitutions are the floor. `check` at 7 s runs in CI gates and pre-commit
hooks.

Reliability: 0.35.2 fixed five platform-specific failures of one class
(`dirname` on BSD and MSYS, `shasum` absent on Git Bash, `$TMPDIR` with a
trailing slash), one intermittent macOS failure stayed undiagnosed, and Windows
CI sharded bats twelve ways because Git Bash cannot run it in parallel. A typed
language with `Result`-based I/O and a standard path API removes the class, not
the instance.

## Product Boundary

Changes:

- The engine implementation, its distribution (prebuilt binaries instead of a
  git clone), and the `update` mechanism.
- Windows support without Git Bash at cutover.

Does not change:

- The CLI surface: commands, flags, exit codes, stdout and stderr text.
- File formats: `agent_sync.yaml`, tool YAML, `.ai/.sync-manifest`,
  `.ai/.template-manifest`, `.ai/backups/<id>/` layout, the `.gitignore`
  managed block.
- Shipped templates and skills under `lib/templates/`; they are embedded in the
  binary unchanged.
- `lib/templates/guard/claude.sh` and the hook snippets `setup-hooks` and
  `shell-init` emit: they run in the user's shell, not in the engine.

## Considered Approaches

### Keep Bash, keep optimising

Rejected. 0.35.1 already took the cheap wins. What remains is the fork cost of
the language itself and a bug class that grows with every platform.

### Big-bang rewrite in a separate repository

Rejected. 416 commits in seven months and a feature release (0.35) three weeks
old: freezing that for the months a rewrite takes is how rewrites die. Nothing
would be verifiable until the end.

### Strangler: Bash dispatcher, commands move one at a time

Chosen. `bin/agentsync.sh` stayed the entry point and delegated each command to
the binary once that command was ported. Releases kept shipping throughout, a
feature landed in whichever implementation owned the command, and every step
was verified by the existing suite. The dispatcher did its job through 0.37.0
and was deleted in Phase 6; the section below records how it worked.

### Language

Rust. Confirmed at the Phase 1 gate on 2026-09-13 — the evidence is in that
plan's completion receipt, and the Go fallback is closed. The original reasons,
from the discussion that produced this document:
`Result` forces every filesystem failure to be handled, `Path` is typed, the
workload (files in, strings transformed, files out; no concurrency) is the easy
part of the language, and cargo-dist produces the `curl | sh` installer,
checksums, and release workflow. Go was the fallback had velocity at the end of
Phase 1 been unacceptable: the dispatcher, the parity suite, and the CI shape
are language-agnostic, and only the crate would have been replaced.

## Architecture

### Layout

```text
Cargo.toml                 # crate `agentsync`, version equal to VERSION (release bumps both)
src/main.rs                # args → run(); exit codes; the only process-aware file
src/lib.rs                 # module tree; engine_version()
src/cli/<command>.rs       # one file per command: args → core calls → text
src/<module>.rs            # core: yaml_subset, project, catalog, tool, payload, style, …
lib/templates/             # unchanged; embedded via include_dir!
lib/templates/guard/claude.sh, install.sh   # the shell floor (see below)
tests/*.rs                 # the whole suite since Phase 7: one file per command surface
tests/common/mod.rs        # the fixtures the integration tests share
```

Until Phase 6 the tree also held `bin/agentsync.sh` (the dispatcher),
`lib/*.sh` and `lib/helpers/*.sh` (the Bash engine), `tests/native_dispatch.bats`
(dispatcher gating), and `tests/native_parity.bats` (Bash vs native diff per
ported command). All of it is readable at tag `0.37.0`, for example
`git show 0.37.0:lib/helpers/backup.sh`; the module docs in `src/` name the
Bash function each file was ported from.

### Dispatcher (Phases 1–5, deleted in Phase 6)

`bin/agentsync.sh` gained `_native_try`, called after `check_for_updates` and
the `--help` interception, before the command `case`. From Phase 5d
`check_for_updates` ran in Bash only when `_native_will_serve` said the
binary would not answer the command; the binary printed the same notice for
the commands it served.

- `AGENTSYNC_NATIVE=0` — always Bash.
- `AGENTSYNC_NATIVE=1` — require a binary; fail loudly without one.
- unset — use a binary when one is found.
- `AGENTSYNC_NATIVE_BIN` — an explicit binary; otherwise
  `<engine>/target/release/agentsync[.exe]` (developer build) or
  `<engine>/bin/agentsync-native[.exe]` (Phase 5 installer).

The dispatcher passed `AGENTSYNC_ENGINE_VERSION=$VERSION`; a binary whose
embedded `VERSION` differed refused to run, so a stale developer build could
never answer for a newer engine. That guard left with the dispatcher.

`_NATIVE_COMMANDS` was the single list of ported commands. A command was ported
when its bats file passed with `AGENTSYNC_NATIVE=1` and its parity tests passed.

### Conformance

While both engines existed, three layers, one seam:

1. **bats with `AGENTSYNC_NATIVE=1`** — the existing suite, unchanged, run
   against the binary for ported commands. `tests/test_helper.bash` defaulted
   `AGENTSYNC_NATIVE` to `0` so a stray developer build never changed what the
   suite exercised.
2. **Parity tests** — `tests/native_parity.bats` ran each ported command
   through both engines on the same fixture and diffed stdout+stderr and the
   exit status. Fixtures were added per command as it was ported.
3. **Golden outputs** — from Phase 2, the Bash engine generated the outputs for
   this repository's own `.ai/src/` with all 13 tools enabled; the native
   engine had to reproduce all 272 files byte for byte. 0.35.1 used the same
   technique.

From Phase 6 the suite had one engine to grade, and all three layers are now
history. `cargo test` is the suite: unit tests inside the crate for pure
modules, and integration tests in `tests/*.rs` that drive the binary Cargo
builds, over the harness in `tests/common/mod.rs`, on Linux, macOS, and
Windows.

### Config reading

The Bash engine never parsed YAML. Its `lib/helpers/yaml.sh` was a
line-oriented reader with its own rules: the first duplicate key wins, an
unquoted value ends at the first `#`, `\n` inside quotes stays literal until
`printf '%b'` at write time, an empty block list keeps scanning and picks up
the next dash list in the file. Every shipped and user config was written
against those rules, and the suite asserts on them.

`src/yaml_subset.rs` therefore ports that reader line for line rather than
adopting a YAML crate. This kept parity provable and keeps the port small.
Replacing it with a real parser plus `doctor` validation is a post-cutover
decision, taken with the quirk list below in hand.

### Errors and output

- Core modules return `Result<_, agentsync::Error>` (`thiserror`). `main.rs`
  maps errors to the same messages and exit codes the Bash command used.
- Bash had two output voices and both are kept: `style` mirrors
  `cli_colors.sh` (bold/green/cyan/yellow/red/dim, decided once from stdout
  being a TTY and `NO_COLOR`), and `log` (Phase 2) mirrors `logging.sh`
  (`[INFO]`, `[WARNING]`, the `═` separator, emoji only when coloured).
- Prompts read the terminal directly (`/dev/tty`, `CONIN$`), as
  `prompts.sh` did, so captured output never breaks interaction.
- A broken pipe on stdout exits 0 silently, matching a shell pipeline.

## Phases

Each phase ends with the full suite green: in both modes while both engines
existed, against the binary alone from Phase 6. Phase 1 has an
executable plan; each later phase gets its own plan when its turn comes,
written against what the previous phase revealed.

Every phase lands on one long-lived branch, and nothing is released until the
cutover. A half-migrated engine has nothing to offer a user — the binary does
not ship before Phase 5, so a release from Phase 1 through 4 would carry the
same Bash engine under a new version number, while asking every install to
absorb the churn. The first release is Phase 5's, the one that replaces the
git clone with a binary. Phase 6 and Phase 7 release as ordinary versions
after it.

### Phase 1 — Foundation and `list`

Crate scaffold, CI on three platforms, dispatcher and its tests, `yaml_subset`,
project config and the layered tool resolver, payload discovery, `version` and
`list` served natively, parity suite. No user-visible change apart from one
prefactor the parity fixtures exposed before a line of Rust existed: `list`
exits 1 silently when the last `.ai/src/tools/*.yaml` override lacks
`enabled: true`, because `list_legacy_enabled_tools` returns the status of its
final `[[ ]]`; it is fixed with a regression test first, so the Bash reference
is correct. Exit: Phase 1 plan's completion receipt; decision gate on language
velocity.

### Phase 2 — Render core and `check`

Port everything `sync` needs to compute outputs without writing them:
`paths` (normalisation, containment, existing-ancestor canonicalisation),
`filters`, full tool resolution (`get_tool_filter`, `resolve_payload_source`
with the legacy warning), `profiles`, `shared` overlays and the engine-owned
skill layer, `rule_operations` (headers, frontmatter merge, `append_imports`,
`merge_to_file`, the three inliners, commands as skills, guard),
`format_conversion` (TOML, Amazon Q JSON, OpenCode MD), OpenCode JSON
composition. The renderer produces an in-memory map of relative path to bytes.

`check` becomes render + compare against the manifest paths, with the same
messages and exit codes as `lib/check.sh`, and no `tar` or temp workspace.
Exit: `tests/check.bats`, `sync.bats`-derived golden outputs, `base_skills`,
`shared`, `profiles`, `opencode`, `resource_resolver` fixtures byte-identical;
`check` on the 13-tool fixture measured and recorded.

### Phase 3 — `sync` transaction and `rollback`

Manifest load/drift/write, backup create/restore/prune in the existing
`.ai/backups/` format (so a Bash `rollback` on an older install still reads
it), the `.gitignore` block, `post_sync` with the install-dir `config.yaml`
gate, `--dry-run`, `--force`, `--only`, `--skip`, `--profile`, `--if-stale`,
`--workspace`, the version-pin gate, the baseline-replacement warning, and the
signal-safe restore. Exit: `sync`, `check`, `rollback` bats files and
`drift`, `outputs_mode`, `team_workflow`, `workspace`, `version_pin` green
natively; `sync` on the 13-tool fixture measured and recorded.

### Phase 3b — Bash 0.36.0 behaviour in `sync`, `check`, `list`, and `rollback`

Release 0.36.0 changed ported commands in Bash on `main` while Phase 3 was
closing, and `main` was merged into the migration branch afterwards. Each family
below gets its own plan, in this order, because the later ones read the config
the first one selects:

1. **Config selection and the version pin.** `project_config_path_r` (an
   explicit `AGENTSYNC_CONFIG_PATH` never falls back), the configless-sync
   refusal, `version_pin.mode: warn | strict` with its scalar shorthand, and
   `check`'s committed-mode rule for `gitignore.update: false`, in `sync`,
   `check`, and `list`.
2. **Backup retention.** `backup.retention: bounded | preserve`, validated
   before `init`, `sync`, and a rollback restore write, skipped by `check` and
   `rollback --list`.
3. **Sources outside the project.** Explicit `source.*` roots trusted through
   `AGENTSYNC_EXTERNAL_SOURCE_ROOTS`, refused roots, `source.tools` for tool
   YAML and payloads, and the refusal of source symlinks that escape the
   project.
4. **The rollback witness.** `after.tsv` (`post-state-v2`) sealed after `init`,
   `sync`, and `rollback`, the preflight that names the first changed path,
   `rollback --force`, and unsealed snapshots restored with a warning.

Exit: `config_safety`, `version_pin`, `backup_retention`, `source_overrides`,
and `rollback_preflight` green natively, `tests/native_parity.bats` green in
both modes, and each family's parity fixtures in place.

### Phase 4 — Remaining commands

Grouped by the module they share, each group its own plan:

- `yaml_edit` family: `enable`, `disable`, `customize`, `simplify`, `show`,
  `diff`, `resolve`, `profile`, `upgrade-config`.
  Planned in four slices: 4a `enable` and `disable` with `yaml_edit` and
  `edit_paths`; 4b `customize`, `show`, and `diff`; 4c `simplify` and
  `resolve` with `snapshot`; 4d `profile` and `upgrade-config`.
- `template_manifest` family: `init`, `refresh`, `dedupe`, `migrate`, `adopt`.
  Planned in five slices: 4e `dedupe` with the template hash, the template
  set, and the parent walk; 4f `adopt`; 4g `migrate`; 4h `refresh`; 4i `init`.
- Standalone: `doctor` (keeps its tri-state exit code), `add`, `export`,
  `import`, `generate`, `shell-init`, `setup-hooks`.
  Planned in four slices: 4j `doctor`; 4k `add`; 4l `export` and `import`;
  4m `generate`, `shell-init`, and `setup-hooks`.

`update` and `release` are rewritten in Phase 5 because their mechanics change.
Exit: `_NATIVE_COMMANDS` lists every command; the whole suite passes with
`AGENTSYNC_NATIVE=1`.

### Phase 5 — Distribution and cutover

- cargo-dist 0.32.0 (`dist-workspace.toml`, `[package.metadata.dist]`): Linux x86_64
  and aarch64 (musl), macOS x86_64 and aarch64, Windows x86_64; the generated
  release workflow runs on `workflow_dispatch` with the tag, `curl | sh` and
  PowerShell installers into `~/.agentsync/bin`, sha256 sums, artifact
  attestations.
- `install.sh` stays hand-written (5e): it detects the cargo-dist target,
  downloads `agentsync-<target>.tar.xz` (`.zip` on Windows) and its `.sha256`
  from GitHub Releases through `curl -w %{http_code}`, verifies the sum with
  `sha256sum` or `shasum`, unpacks it through `tar` into
  `~/.agentsync/bin/agentsync`, and links it; `AGENTSYNC_VERSION=<tag>` still
  pins, and a pinned tag whose archive answers 404 installs from source as
  before (decision 3). The cargo-dist installers ship alongside it. The
  binary's install needs no `AGENTSYNC_HOME`; a source install still gets it.
- `update` replaces the binary from GitHub Releases and keeps `update <version>`
  pinning; the `agentsync_version` gate is unchanged. The binary's `update`
  (5d) downloads `agentsync-<target>.tar.xz` (`.zip` on Windows) and its
  `.sha256` through `curl`, verifies the sum in-process, unpacks through
  `tar`, asks the new binary for `version` and `__catalog`, diffs the two
  embedded catalogs against the project's overrides, and renames the new
  binary over the running one; the changelog comes from the archive. The
  update banner reads `.update_cache` beside the install's `bin/`, refreshed
  by a detached `__update-cache` run from `releases/latest`. `update` is not
  in `_NATIVE_COMMANDS`: a checkout keeps Bash's git-based `update` until
  Phase 6, and `tests/update_native.bats` runs the binary directly.
- `release` bumps `VERSION`, `Cargo.toml`, and `Cargo.lock` together; the
  auto-tag workflow triggers the release build.
- The installed `agentsync` link points at the binary. `bin/agentsync.sh`
  stays the dispatcher in the repository, the parity harness, until Phase 6
  deletes it; `_native_bin` also accepts `<engine>/bin/agentsync[.exe]`, the
  name the installers and the source-install switch use (5e). A source
  install moves to the binary through Bash's `update` (5e,
  `_update_switch_to_binary`): after the git reconcile it downloads and
  verifies the archive of the new version, places
  `<install>/bin/agentsync[.exe]`, and points the `agentsync` link at it; a
  version without an archive is silent, any other failure is a warning and
  the checkout keeps running. Until then, on a terminal, `check_for_updates`
  prints one dim line naming `agentsync update` as the way to the binary.
- Windows: the binary is the entry point; the native CI job runs the bats
  suite against it serially and unsharded (5e). Terminal colour on legacy
  consoles is enabled with the `anstream` crate if needed.

Planned in five slices, each its own plan, in this order because each one
builds on the one before:

- 5a `help` and the answers `bin/agentsync.sh` gives before it delegates: the
  usage, the `--help` interception, and the unknown-command refusal.
- 5b `release` with the crate version: `Cargo.toml` and `Cargo.lock` carry
  `VERSION`, and `release` bumps the three together.
- 5c the release build: cargo-dist, the five targets, the release workflow,
  the installers, checksums, and attestations. A tag the auto-tag workflow
  pushes with `GITHUB_TOKEN` starts no other workflow, so the auto-tag
  workflow dispatches the release build. The repository is
  `yelmuratoff/agent_sync`; `yelmuratoff/agent`, which `install.sh` and the
  README named when this slice was planned, redirects to it, and no GitHub
  release existed yet.
- 5d `update` and the update notice: the binary replaced from GitHub Releases,
  `update <version>`, `--strict` conflicts from the catalogs embedded in the
  old and the new binary, the changelog embedded, and `check_for_updates` with
  the format notice moved into the binary.
- 5e the cutover: installs link the binary, existing clone installs move to
  it, Windows runs the suite against the binary without sharding, and the
  README and `.ai/src/AGENTS.md` describe a single static binary.

Decisions for Phase 5, taken on 2026-09-16 when the maintainer left them to
the plan author:

1. Downloads spawn `curl` and `tar`, as `import` does, and the checksum is
   verified in-process with `sha2`; no HTTP crate is added. Windows 10 and
   later ship both executables.
2. A clone install moves to the binary through `update`: in a clone without a
   binary, the cutover release's `agentsync update` downloads and verifies the
   binary and re-links, and until then an interactive command prints a
   one-line notice. The clone keeps running Bash meanwhile.
3. A pin to a release older than the first binary still installs: `install.sh`
   keeps its git-clone path for those tags until Phase 6, so it stays
   hand-written (it downloads and verifies the binary otherwise) while the
   cargo-dist installers ship alongside it. `update <tag>` from a binary
   install refuses such a tag and prints the installer command that pins it.
4. `bin/agentsync.sh` is not reduced to a shim in Phase 5: the parity suite
   needs the Bash reference until Phase 6, and installs link the binary
   directly.

Exit: first binary release; README and `.ai/src/AGENTS.md` updated; the
"pure Bash" claim replaced by "single static binary".

### Phase 6 — Retire Bash

Delete `lib/*.sh` and `bin/agentsync.sh`; port the Bash-unit bats files
(`files`, `paths`, `backup`, `tmp`, `gitignore`, `update_snapshot`) to Rust
unit tests; keep the CLI-level bats files as the conformance suite until
Phase 7 retires them; remove `_native_try`, `AGENTSYNC_NATIVE`, and the
Windows shard matrix; ShellCheck covers only the guard and installer scripts
that remain.

Closed in `docs/plans/2026-09-19-rust-migration-phase-6-retire-bash.md`.
The suite ran against the binary on Linux, macOS, and Windows; the Windows
shard matrix stayed for the binary because Git Bash cannot run `bats --jobs`
(a deviation recorded in that plan), until Phase 7 retired bats and the shards
with it. The Bash engine is deleted, ShellCheck lints `install.sh` and
`lib/templates/guard/claude.sh`, and `scripts/perf/bench.sh` compares against a
0.37.0 checkout named by `AGENTSYNC_BASH_CLI`.

### Phase 7 — Retire bats

The CLI-level conformance suite (41 `.bats` files, 727 cases at the start of
Phase 7) is now Rust integration tests on `assert_cmd`, the shape
`tests/cli.rs` already used: one commit per bats file, Rust test names copied
from the bats test names so a reviewer maps them one to one.
`tests/common/mod.rs` carries the fixtures `tests/test_helper.bash` gave the
suite. That helper is deleted, bats and GNU parallel are gone from CI, and the
Windows sharding scaffolding with them — one `test` job now runs the Rust
gates on Linux, macOS, and Windows alike.

This could not move earlier. The suite was the only proof of parity while both
engines existed: the same test graded Bash under `AGENTSYNC_NATIVE=0` and the
binary under `=1`. Rewriting it before Phase 6 would have replaced the contract
with its own reimplementation. With Bash gone there is no second engine to
grade, the argument expired, and bats was a dependency that cost a sharded
Windows run.

Exit, met: `cargo test` is the whole suite; no `.bats` file remains.

### The shell floor

Two shell scripts survive every phase, because a binary cannot do their job:

- `lib/templates/guard/claude.sh` (50 lines of POSIX `sh`) — it runs in a
  teammate's checkout where the CLI is not installed, which is the reason the
  hook exists. The alternative is committing a per-platform binary into the
  user's repository.
- The installer — bootstrap: something must detect the platform and fetch the
  binary before a binary exists. From Phase 5 cargo-dist generates it, so it
  stops being hand-maintained code. `rustup` ships the same way.

The text `shell-init` and `setup-hooks` emit stays shell because the shell
`eval`s it and Git runs it as a hook; from Phase 4 the logic that produces that
text is Rust, and the emitted snippet is a wrapper that calls the binary.

## Distribution

Before Phase 5 `curl | bash` cloned the repository into `~/.agentsync` and
symlinked `bin/agentsync.sh`. Since 0.37.0 the same command downloads a
platform binary, verifies its sha256, and links it; `agentsync update` swaps
the binary. The `agentsync_version` pin, `update <version>`, and
`AGENTSYNC_VERSION=<tag>` in the installer keep working, which keeps the
committed-outputs team workflow intact; a pin to a tag older than 0.37.0 still
installs from source, since those tags have no archive.

Until Phase 5 no user had a binary: native code path exposure was limited to
developers who ran `cargo build --release`, and the dispatcher's default fell
back to Bash when no binary existed.

## Known quirks to reproduce now and fix after cutover

Recorded so the parity work reproduces them knowingly and the post-cutover
cleanup has a list.

**Status, 2026-09-20:** the cutover is past.
`docs/plans/2026-09-20-post-cutover-quirks.md` triages every item — 41 defects,
3 compatibility decisions, 10 cosmetic — and records what each would cost to
fix. Items 22, 24, 44, 45 and 55 are fixed in 0.38.0 and struck below; item 8
was never a quirk. The rest stand, and 21 of them have no test pinning them,
which the plan puts first. Nothing here is a regression: each item is a
decision to make once rather than a bug to find twice.

1. *Fixed after 0.42.0.* `parse_yaml_list` on an empty block key kept scanning
   and returned the next dash list anywhere later in the file
   (`yaml.sh:180-197`); the first line under the key that is not a list item
   now ends the list.
2. *Kept and documented after 0.42.0.* An unquoted scalar is cut at the first
   `#`, even without a preceding space. Changing it would lengthen every
   existing unquoted value holding a `#` without a migration, so the README
   tells users to quote such values instead.
3. *Kept after 0.42.0.* `\n` in a quoted header stays literal until
   `printf '%b'` at write time. The only place it shows is `show`, which then
   prints a header on one line; unescaping in the reader would make the writer
   interpret the backslashes twice.
4. *Fixed after 0.42.0.* `get_tool_value` could not override a base value
   with an empty string, and never consulted `base:` when a shipped file
   existed for the slug. A key the user file writes now wins even empty, and
   a declared `base:` fills what the shipped file leaves empty.
5. *Fixed after 0.42.0.* `defaults.enabled` in `agent_sync.yaml` and the
   `defaults:` block in `lib/config.yaml` were never read. `init` no longer
   writes the key and the shipped block is gone; `defaults.cleanup` in the
   project config is read and stays.
6. *Fixed after 0.42.0.* Bash `printf '%-Ns'` padded styled strings including
   their escape bytes, so coloured `list` columns drifted; `pad_right` now
   counts only the cells a terminal shows.
7. *Kept, de-duplicated after 0.42.0.* `outputs` absent means `local`, except
   when `gitignore.update: false`, which means `committed`. The rule stands:
   dropping it would flip a project relying on it to local outputs. It lived
   in three files and now lives in `project_config::outputs_mode`.
8. *Struck 2026-09-20: not a quirk.* Locale-ordered tool listings were
   ratified as an accepted deviation, below; the numbering stays as it is
   because source comments and tests cite these items by number.
9. *Fixed after 0.42.0.* `read_frontmatter_field` returned the last occurrence
   of a key, and read `"x" # note` as `x"`. It now returns the first, and a
   quoted value up to its closing quote.
10. *Fixed after 0.42.0.* `_rule_paths_csv` collected every list item in a
    rule's frontmatter once it had a bare `paths:` key, not only the items
    under `paths:`. It now reads only the `paths:` value, and accepts a flow
    list or a single glob there as well as a block list.
11. *Fixed after 0.42.0.* The inline skill index stripped `>` from
    `description: >-` and indexed the skill with the description `-`. It now
    reads the description as `skills show` does, a block string folded to one
    line.
12. *Fixed after 0.42.0.* `sync --workspace` reported the status of the last
    project that failed as "max exit code" and exited with it
    (`bin/agentsync.sh`, `cmd_workspace_fanout`).
13. *Fixed after 0.42.0.* `version_pin: warn` followed later by a
    `version_pin:` mapping with `mode: strict` read as `warn`: the reader
    answered the first `version_pin` key, so the nested lookup was empty and
    the scalar won (`version.sh`, `version_pin_mode`). A key holding a value
    no longer opens a section, so the later mapping answers.
14. *Fixed after 0.42.0.* `enable` and `disable` edited `.ai/agent_sync.yaml`,
    or a root `agent_sync.yaml`, even when `AGENTSYNC_CONFIG_PATH` selected
    another file, while "already enabled" read the selected one.
15. *Fixed after 0.42.0.* `disable` created `.ai/agent_sync.yaml` when the
    project had none.
16. *Fixed after 0.42.0.* `enable` under a `tools:` block without `enabled:`
    appended a second `tools:` block at the end of the file; it now adds
    `enabled:` inside the existing block. The same change makes a nested key
    match only a direct child, where `tools.enabled` used to find
    `tools.foo.enabled` in both the reader and the editor.
17. *Fixed after 0.42.0.* `disable` listed every argument that was not
    enabled afterwards, unknown slugs and repeated ones included.
18. *Fixed after 0.42.0.* `diff <slug>` printed "No user overrides" and exited
    0 when no tool had an override, whatever the slug.
19. *Fixed after 0.42.0.* `show <slug> <resource>` labelled an override
    `base` when its extension differed from the shipped template's.
20. *Fixed after 0.42.0.* `diff` selected the project config before it
    validated the resource; `customize` and `show` validate first.
21. *Fixed after 0.42.0.* `simplify`'s payload pass scanned `.ai/src/tools` even when `source.tools`
    moves the tool override directory.
22. *Fixed in 0.38.0.* `resolve` without a terminal ignored its tool filter
    and exited 0, having already cleared the queue; it is read-only now.
23. *Fixed after 0.42.0.* `yaml_remove_key` (`simplify --apply`, `resolve`
    adopt) dropped the blank lines directly after the removed block. They stay
    when a sibling came before the block and no blank line already separates
    it from what follows.
24. *Fixed in 0.38.0.* `resolve` in a project without overrides deleted
    `.ai/.pending-resolutions.yaml` whether or not it had a terminal.
25. *Fixed after 0.42.0.* `simplify --apply` without a terminal deleted
    byte-identical payload copies but kept an override file it emptied. Off a
    terminal it now deletes neither without `-y`, and says so.
26. *Fixed after 0.42.0.* `profile add --tools` kept spaces around
    comma-separated names and accepted unknown tools: `'claude, codex'` wrote
    `.ai/src/tools/ codex-hub.yaml`.
27. *Fixed after 0.42.0.* `profile add` wrote the profile's `tools:` list as
    `[a,b]`, without spaces; it writes `[a, b]`.
28. *Fixed after 0.42.0.* `profile add <name> --tools` with no value exited 1
    without a message.
29. *Fixed after 0.42.0.* `profile remove` deleted the whole config home, an
    adopted one included, along with anything the tool itself kept there. It
    now removes the files the manifest records and the directories that
    leaves empty.
30. *Fixed after 0.42.0 for `AGENTSYNC_CONFIG_PATH`.* `upgrade-config`
    rewrote the default config whatever the variable selected. It still
    rewrites every top-level `agentsync_version:` line: the reader takes the
    first, and rewriting the others keeps no stale pin behind.
31. *Fixed after 0.42.0.* `dedupe` removed a top-level source directory it
    emptied, such as `.ai/src/rules/`, not only emptied skill folders. It now
    stops below that directory; an emptied skill category still goes.
32. *Fixed after 0.42.0.* `dedupe` ignored `AGENTSYNC_CONFIG_PATH`, even a
    missing one: it read `shared.path` from and appended declined entries to
    `.ai/agent_sync.yaml`, else a root `agent_sync.yaml`.
33. *Fixed after 0.42.0.* `adopt` of a merged rules file such as Zed's
    `.rules` answered that it was not a recognised output; the merge refusal
    was reachable only for a file inside a rules directory.
34. *Fixed after 0.42.0.* `adopt --all` printed one `✓ adopted` line per
    destination, so two identical edits of one source named it twice.
35. *Fixed after 0.42.0.* `migrate --apply --yes` printed `removed .agent/
    (pre-v0.6 layout)` with no blank line before `Planned moves:`.
36. *Fixed after 0.42.0.* A legacy file without an extension, such as
    `.ai/src/settings/README`, moved to `.ai/src/tools/README/settings.README`.
    It is no payload, so `migrate` leaves it and the `update` banner ignores it.
37. *Kept after 0.42.0.* Off a terminal without `--yes`, `migrate --apply`
    consolidates identical MCP files but leaves `.agent/` in place. Each step
    takes its prompt's default there: consolidating identical files loses
    nothing and defaults to yes, removing `.agent/` loses its content and
    defaults to no.
38. *Kept after 0.42.0.* `refresh` heals `.ai/.template-manifest` with every
    shipped template that matches its copy, including categories outside
    `--only` and `AGENTS.md` without `--include-agents-md`. The healing keeps
    the manifest honest (`src/config/template_manifest.rs`); scoping it would
    rewrite that committed file on every scoped refresh and buy nothing.
39. *Fixed after 0.42.0.* Off a terminal without `--yes`, `refresh` applied
    pending auto-updates, because the TTY gate looked only at new files and
    conflicts; with `--include-deleted` and nothing else pending it printed
    each RESTORE prompt on stderr and declined it. The gate now refuses any
    pending change there.
40. *Fixed after 0.42.0.* `refresh` read `template_overrides` from
    `.ai/agent_sync.yaml`, else a root `agent_sync.yaml`, ignoring
    `AGENTSYNC_CONFIG_PATH`.
41. *Fixed after 0.42.0.* A detected tool whose destination had a file where a
    directory was expected, such as a legacy single-file `.clinerules`, made
    `init` refuse at the backup step with `Backup target parent is not a
    directory`. Cline no longer writes below `.clinerules`, and the refusal
    that remains for any other such file names it and says to move it.
42. *Fixed after 0.42.0.* `init` dropped every space inside a `--tools` or
    `--content` token, so `cla ude` read as `claude`; `--tools=` and
    `--content ''` skipped the wizard while contributing nothing. Tokens are
    now trimmed, a tool name with a space inside is refused, and an empty
    value is refused as a missing one.
43. *Fixed after 0.42.0.* `init` healed `.ai/.template-manifest` before it
    adopted existing outputs, so an adopted `AGENTS.md` carried the template's
    hash and `refresh` treated it as a silently kept edit. It heals after
    adopting now.
44. *Fixed in 0.38.0.* `doctor`'s secret scan listed only the lines of the
    first pattern with a hit, so an AWS key on line 1 hid an OpenAI key on
    line 2.
45. *Fixed in 0.38.0 for `${…}`, after 0.42.0 for `<…>`.* A line holding
    `${…}` anywhere was never reported, however real the key beside the
    placeholder, and a line holding `<…>` was never reported unless it held
    `sk-`. The scan now reads the line with those spans removed; a `<…>` span
    that holds `sk-` stays readable.
46. *Fixed after 0.42.0.* `add mcp` re-emitted only the `mcpServers` member of
    `.ai/src/mcp.json`, dropping every other top-level member, and replaced a
    file without a `"mcpServers"` substring with a fresh object holding the
    one server. Every top-level member now stays in order, and `mcpServers` is
    added beside them when missing.
47. *Fixed after 0.42.0.* `add mcp` took the first `"mcpServers"` anywhere in
    the file as the member, so a nested decoy made the merge fail with
    `failed to update`; only the top-level member counts now.
48. *Fixed after 0.42.0.* `add mcp` stopped reading the server map at the
    first key that is not a string and dropped the servers after it. A file
    that is not a valid JSON object is now refused and left unchanged.
49. *Fixed after 0.42.0.* `add mcp` created `.ai/src/mcp.json` with an empty
    server map before it validated `--env`, so a bad pair left the file
    behind; `--args` and `--env` read only the first line of their value.
50. *Fixed after 0.42.0.* `import` stripped a `.git` suffix before a trailing
    `/`, so `https://github.com/user/repo.git/` downloaded the repository
    `repo.git`.
51. *Fixed after 0.42.0.* A directory `import` copied the source project's
    `.ai/` alone, so a `source:` override pointing elsewhere in that project
    was not carried; no import read `source.*` at all, and the imported config
    kept paths the target project lacked. A directory import now copies what
    `export` would archive, every import reads each section from its declared
    relative path, and the imported config drops the `source.*` entries that
    name another place than where the import writes.
52. *Fixed after 0.42.0.* `generate` ended with status 1 and no message when
    stdin closed before the menu choice or the description was complete. At
    the menu it now says `Cancelled.`; in the description the end of input
    ends the text, which then goes into the prompt.
53. *Fixed after 0.42.0.* `setup-hooks` read its options in order and refused
    the first unknown one, so `--bogus --help` printed the unknown-option
    error, not the help.
54. *Fixed after 0.42.0.* `release` exited 1 with nothing after its
    `Continue? [Y/n]:` prompt when stdin ended there; it now says the input
    ended and nothing was released.
55. *Fixed in 0.38.0.* `update`'s changelog renderer matched a
    `## <version>` heading by prefix, so `## 9.9.90` rendered under `9.9.9`
    and ran on to the end of the file.

## Accepted deviations

Appended one line at a time as they are found, with the phase:

- Phase 1: clap rejects unrecognised arguments to a ported command with exit 2;
  Bash ignored them.
- Phase 1: tool slugs sort in byte order; Bash used the locale's `sort`.
- Phase 1: `\r\n` line endings in config files read as `\n`; Bash kept a
  trailing `\r` that normalisation then trimmed, so values agree.
- Phase 2: directory listings and globs read in byte order; Bash globs followed
  the locale's collation, so merged rules, indexes, and imports can order
  differently for names the locale sorts otherwise.
- Phase 2: when `check`'s render fails, the log tail names project paths and
  the virtual `/<agentsync>` and `/<agentsync-overlay>` roots where Bash named
  its random temporary workspace and overlay directories.
- Phase 2: `check` without `.ai/` reports `Incomplete copy — missing: .ai` on
  stderr where Bash printed `tar`'s platform-specific error.
- Phase 2: symlinks under `.ai/` and among the outputs are followed; Bash's
  `tar` copy kept them as links.
- Phase 2: `agent_sync.yaml` and OpenCode JSON that are not valid UTF-8 are read
  with replacement characters; Markdown transforms stay byte-exact.
- Phase 2: `printf '%b'` escapes in rule headers expand as Bash 3.2 does,
  leaving `\u` literal where Bash 5 expanded it.
- Phase 3: `sync`'s step lines name `/<agentsync>/lib/templates` and
  `/<agentsync-overlay>/<layer>/src` where Bash printed the engine checkout and
  its temporary overlay directories.
- Phase 3: an edited install-directory `lib/config.yaml` does not enable
  post-sync hooks; the binary reads the shipped `post_sync.allow: false`, and
  `AGENTSYNC_ALLOW_POST_SYNC=true` enables them as before. Phase 5 did not
  settle a user-level setting and the install directory is gone, so the
  environment variable is the answer: a binary install has no file a user could
  edit to grant this, and the shipped `lib/config.yaml` is compiled in. Adding
  a user-level switch is a feature, not migration work.
- Phase 3: a failed write, copy, or removal reports the Rust I/O error where
  Bash printed the `cp`, `mkdir`, or `rm` message; the status and the restore
  are unchanged.
- Phase 3: `sync` keeps running when nothing reads its stdout and finishes its
  transaction; Bash died of `SIGPIPE` at its next log line and left the run
  half written.
- Phase 3: a trapped `INT`, `TERM`, or `HUP` takes effect at the next step of
  `sync` or once `rollback`'s restore returns; Bash's trap fired after the
  running command. A second signal does not interrupt the restore.
- Phase 3: symlinks inside a synced skill, rule, or command tree are copied as
  the files they point to; Bash's `cp -r` copied the links.
- Phase 3: the guard hook's `chmod +x` adds execute permission for every class,
  as the default umask does, whatever the process umask.
- Phase 3: on a terminal a log message prints as written; Bash's `echo -e` also
  expanded backslash escapes inside it.
- Phase 3b: the native engine hashes `after.tsv` in-process, so a missing or
  failing `sha256sum` or `shasum` neither fails a seal nor leaves a rollback
  unchecked.
- Phase 3b: `after.tsv` records a regular file as `exec` when any execute bit
  is set; Bash's `[[ -x ]]` asked whether the current user may execute it.
- Phase 3b: a backup skips sockets and device files under a target, which
  `tar` and `cp -pPR` recreated or reported; a FIFO is recreated with `mkfifo`.
- Phase 4b: `customize`, `show`, and `diff` name shipped templates as
  `/<agentsync>/lib/templates/...` where Bash printed the install directory.
- Phase 4e: `dedupe` takes the shipped template set from the embedded
  templates where Bash walked `$AGENTSYNC_HOME/lib/templates`.
- Phase 4e: an `AGENTSYNC_REPO_ROOT` that does not exist reports
  `Error: Repository root not found: <path>` where Bash printed `cd`'s message;
  the status is 1 in both, as for the other ported commands that discover a
  project.
- Phase 4f: `adopt`'s OpenCode refusal names a shipped settings file as
  `/<agentsync>/lib/templates/...` where Bash printed the install directory.
- Phase 4g: `migrate` prints the embedded `lib/prompts/migrate.md` where Bash
  read the install directory's copy.
- Phase 4h: `refresh` prints `Templates: /<agentsync>/lib/templates` and
  compares against the embedded templates where Bash printed and read
  `$AGENTSYNC_HOME/lib/templates`.
- Phase 4h: a template `refresh` creates is executable when it starts with
  `#!`, the mode the shipped scripts carry, where Bash's `cp` copied the
  checkout's mode; an existing file keeps its mode in both engines.
- Phase 4h: `refresh` and `migrate` keep an existing `.ai/.template-manifest`
  mode, as `TemplateManifest::write` has since Phase 4g, where Bash's
  `template_manifest_write` lets `sort -o` rewrite the file, which BSD sort
  does through a new `0600` inode.
- Phase 4h: a `refresh` prompt whose terminal device cannot be opened declines
  silently where Bash also printed the shell's `/dev/tty` open error.
- Phase 4i: the multiselect switches the terminal through `stty` (settings
  saved with `stty -g`, restored on exit) where Bash's `read -rsn1` did it
  in-process; a lone Escape registers after the same one-second wait.
- Phase 4i: a failed scaffold write in `init` reports the Rust I/O error where
  Bash printed the shell's redirect message (`init.sh: line N: <path>: Is a
  directory`); the restore and the status are unchanged.
- Phase 4i: `init`'s first sync runs in-process where Bash spawned
  `lib/sync.sh`; the transcript is the same.
- Phase 4j: `doctor` validates JSON in-process as `python3 -c 'json.load'`
  judges it (`NaN` and `Infinity` accepted; a BOM, trailing commas, control
  characters, and an empty file rejected), where Bash's verdict depended on
  `python3`, `node`, or neither being installed.
- Phase 4j: `doctor` lists skill directories, rules, overrides, and parent
  files in byte order (the Phase 2 deviation), so `skills/Zeta/` precedes
  `skills/empty-one/` where the locale's glob put it after.
- Phase 4k: `add --force` onto a destination that is a directory reports the
  Rust I/O error where Bash printed the shell's redirect message (`add.sh:
  line N: <path>: Is a directory`); the status is unchanged.
- Phase 4l: `export`'s archive bytes are not compared; two `tar -czf` runs on
  the same files differ in the gzip header's time, so the parity fixtures
  compare the report and the archive's listing.
- Phase 4l: `import` extracts into a scratch directory under the system temp
  dir, removed when the command ends, where Bash used the run directory.
- Phase 4m: `generate`'s clipboard tip names the first of `pbcopy`,
  `wl-copy`, `xclip`, `xsel` the binary finds on `PATH`, as `command -v`
  found it; the tip is printed only on a terminal.
- Phase 4m: `shell-init`'s refusals are log lines coloured from stdout, as
  `_use_colors` decided, through `Log::capturing`.
- Phase 5b: `release` requires the three `VERSION` components to be decimal
  integers and refuses others with `Cannot parse VERSION`; Bash evaluated them
  as shell arithmetic, so `1.a.0` bumped to `1.a.1` and `1.2.3.4` died with
  the shell's syntax error.
- Phase 5b: `release` reports the Rust I/O error when `git` cannot be started
  or `CHANGELOG.md` cannot be read, where Bash printed the shell's `command not
  found` (status 127) or awk's message (status 2); the tag is missing in both.
- Phase 5b: outside a checkout, `release` falls back to `AGENTSYNC_HOME` alone,
  when it holds a `.git`; Bash also tried the dispatcher's own checkout, which
  the binary has no counterpart for.
- Phase 5d: `update` on a binary install has no git reconcile, no autostash
  line, and no relink warning; it refuses an unknown tag with a link to the
  releases page, a tag older than the binary releases with the installer
  command that pins it, a checksum mismatch, and an archive whose binary does
  not answer `version` or `__catalog`, each with status 1 and the old binary
  kept. `update --help` says "the latest release" where Bash said "the latest
  main".
- Phase 5d: a conflict whose base value is empty before or after the update
  keeps its five columns in the report and the queue; Bash's tab-separated
  `read` collapsed the empty field and shifted the values.
- Phase 5d: the changelog wraps by character count where `fold -s` counted
  bytes (GNU) or columns (BSD); the width still comes from `tput cols`.
- Phase 5d: the update banner's cache is `.update_cache` beside the install's
  `bin/` (`target/.update_cache` for a developer build), refreshed from the
  latest GitHub release rather than the newest tag; the checkout's
  `.update_cache` stays Bash's.
- Phase 6: `release` recognises a checkout by `VERSION` and `Cargo.toml`, and a
  directory without `Cargo.toml` is refused as `Must be run from the AgentSync
  repository.`; Bash looked for `bin/agentsync.sh`, which Phase 6 deletes, and
  reported the missing manifest as a missing crate version.
- After Phase 7: two strings that named the deleted Bash entry point are
  corrected. `check` ends a drift report with `Please run: agentsync sync`
  where it printed `Please run: lib/sync.sh`, and the hooks `setup-hooks`
  installs no longer fall back to `bash lib/sync.sh` when `agentsync` is not on
  `PATH` — that file cannot exist in a user's project any more, so the hook
  says so and skips instead. Both were byte-identical to Bash until here, and
  both would have told a user to run something that does not exist.
- After Phase 7: clap is gone, and with it the Phase 1 deviation. `list`,
  `check`, and `version` ignore extra arguments again as Bash did, and a
  leading `--` reaches the command's own parser (`sync -- --dry-run` is
  `Unknown option: --`, as `sync.sh` answered) where clap had swallowed it.
- After Phase 7: `enable` exits 1 when any named tool is unknown, after
  enabling the ones it knows; Bash exited 0 and only printed the list. A
  script that enables a misspelt slug now learns of it. Every command answers
  `--help` with its own usage in one shape (`output::help`), including
  `check`, `list`, `disable`, `resolve`, and `doctor`, which had answered with
  the top-level usage, and `upgrade-config`, which had run instead.
- After Phase 7: `setup-hooks` rewrites a marked block whose body differs from
  the current one, keeping the rest of the hook; Bash reported any existing
  block as already present, so a hook that fell back to `bash lib/sync.sh`
  survived every upgrade. A block missing its end marker is still left alone.
- After Phase 7: a directory under `skills/` without `SKILL.md` is a category.
  Sync walks it for skills and lands each one flat at `<dest>/<name>/` instead
  of copying the directory. Bash copied every top-level entry as it was, and
  most tools then ignored the nested skills. Two skills sharing a name stop
  sync.
- After Phase 7: `doctor` no longer suggests `agentsync simplify` for an empty
  skill directory. Bash printed that tip, but `simplify` never touched skill
  directories; the advisory itself still says to populate or remove it.
- After Phase 7: `targets.commands.include` and `.exclude` filter a native or
  TOML commands directory too, as `_TEMPLATE.yaml` documents. Bash passed
  empty filters there and applied them only to generated command skills and
  the inlined command index.
- After Phase 7: sync refuses to write, copy, or remove an output through a
  symlink that resolves outside the project, into `.ai/` or `.git/`, or into an
  explicit source root, and a `.gitignore` link that leaves the project. Bash
  followed every link. A link that stays among the outputs is still followed,
  and removing a link removes only the link.
- After Phase 7: with a manifest present, sync renders in memory first and
  stops before writing anything when it would change an existing file the
  manifest does not record; `--force` replaces such files, and `check` and
  `sync --dry-run` report it. Bash overwrote them.
- After Phase 7: cleaning up a disabled tool removes only the files earlier
  syncs recorded under its dests, never one below another enabled tool's
  dest. Bash removed each dest whole.
- After Phase 7: a directory prune with no manifest removes nothing, and an
  unrecorded `command-*` skill folder is kept with the usual `Kept` warning.
  Bash pruned every extraneous entry when no manifest existed and removed
  obsolete command skills unconditionally.
- After Phase 7: a synced skill folder that holds recorded files is updated in
  place: recorded files its source dropped go, files sync never wrote stay.
  Bash replaced the folder whole.
- After Phase 7: `.ai/.sync-manifest` starts with a `# exuno <version>` line,
  and `sync --if-stale` syncs when it names another version or is missing.
  Bash wrote no header and judged staleness by file times alone.
- After Phase 7: a `base:` variant inherits the project's override of its base
  tool, except `name`, `base`, `profile_home`, `enabled`, `post_sync`, and each
  target's `dest`, `legacy_dest`, `source`, `user_dirs`, and `ownership`. Bash
  read only the shipped base.

## Risks

- **Windows during the strangler.** Under Git Bash, POSIX paths in
  `AGENTSYNC_REPO_ROOT` and `TMPDIR` do not reach a native executable. Native
  bats runs on Windows waited until Phase 6, when the suite moved to the
  binary and the engine's path model became drive-aware; `cargo test` covered
  the Rust side on Windows from Phase 1.
- **Symlinked project roots.** Bash keeps the logical `$PWD`; Rust's
  `current_dir()` is physical. Phase 2's `paths` module honours `$PWD` when it
  names the same directory, so manifest and display paths do not change.
- **Interactive prompts.** Bash reads `/dev/tty`; the port must do the same or
  captured-output flows (`check_for_updates`, hooks) change behaviour.
- **Feature work during migration.** A feature on an unported command landed in
  Bash; after Phase 3 a feature on `sync` landed in Rust only. The dispatcher's
  list made the owner explicit. From Phase 6 every feature lands in Rust.
- **Maintainer velocity.** The decision gate at the end of Phase 1 existed for
  this and was answered on 2026-09-13: Rust stays, Go is off the table.

## Definition of Done

- `bin/agentsync.sh` and `lib/*.sh` are gone; `agentsync` is one binary on five
  targets, installed and updated from GitHub Releases.
- `cargo test` is the whole suite and passes on Linux, macOS, and Windows in
  one unsharded job per platform.
- Golden outputs for this repository's `.ai/src/` with 13 tools are
  byte-identical to 0.35.2's.
- `install.sh`, `update`, the version pin, and the guard hook behave as
  documented in the README, which no longer says "pure Bash".
