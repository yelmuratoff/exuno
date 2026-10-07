---
paths:
  - "tests/**"
  - "src/**"
---

# Testing Rules

Tests exercise the in-tree binary against local fixtures only. Behaviour names, deterministic setup, clean teardown.

## Framework

- `cargo test` is the whole suite: unit tests inside `src/` and integration tests in `tests/*.rs` (`assert_cmd`, `predicates`, `tempfile`).
- One integration file per command surface — `tests/sync.rs`, `tests/refresh.rs`, `tests/profiles.rs`, and similar focused files. `tests/cli.rs` covers the entry point itself.
- Integration tests run the real binary through `env!("CARGO_BIN_EXE_exuno")`, so no release build is needed first.
- Run everything with `cargo test`, or one surface with `cargo test --test <file stem>`.

## Test Structure

- `tests/common/mod.rs` is the shared harness. `Project::empty()` is a temp directory with `git init` and a test identity; `Project::seeded(&[])` adds `exuno init`.
- `project.exuno()` returns an `assert_cmd::Command` in the project with the developer's `EXUNO_*` and `AGENTSYNC_*` variables removed and git pointed at an absent global and system config.
- The harness also carries `write`, `append`, `read`, `exists`, `join`, `path`, `sha256`, `git`, and `enable_tools`, plus `unreadable_dirs_are_possible()` and `chmod()` for permission cases.
- A fixture only one file needs stays a plain function in that file; the harness grows only when a second file needs the same helper.
- A pure function's behaviour is a unit test in its own module; a command's observable contract — exit status, output, files on disk — is an integration test in `tests/`.

## Conventions

- Name tests by behaviour verified: `fn sync_refuses_to_overwrite_a_manual_edit()`, not `fn test_sync()`.
- Assert on the stream the binary writes to — `.stdout(...)` or `.stderr(...)` — never a merged stream.
- Keep tests hermetic: temp directories only, no network, no real GitHub calls.
- Protect the developer environment from side effects such as clipboard writes, global config changes, or edits outside the temp project.
- A test that cannot run on Windows is `#[cfg(unix)]` with a comment naming the quirk: a shell script standing in for a program the binary spawns, a FIFO, a pty, a POSIX hook, `chmod` bits.

## When Adding a New Tool

- Add sync assertions to `tests/sync.rs` verifying output files exist.
- Add filter tests to `tests/sync_options.rs` for `--only` / `--skip`.
- Add check assertions to `tests/check.rs`.

## CI

- `test` runs fmt, clippy, `cargo test`, and the release build on Linux, macOS, and Windows. A failure on one platform points to a portability issue first.
- ShellCheck runs separately on `install.sh` and `lib/templates/guard/claude.sh`: `shellcheck -x -S warning -e SC1091`.
