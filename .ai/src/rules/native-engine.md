---
paths:
  - "src/**"
  - "Cargo.toml"
  - "tests/*.rs"
---

# Engine Rules

The Rust crate at the repo root is the whole engine: one binary, `exuno`, built from `src/` with the templates embedded. The Bash engine it replaced last shipped in 0.37.0 (`docs/specs/2026-09-12-rust-migration-design.md`); `cargo test` now carries its behaviour as the contract.

## Toolchain

- Edition 2024, `rust-version = "1.85"`, `unsafe_code = "forbid"`. `cargo fmt --all --check` and `cargo clippy --all-targets -- -D warnings` stay clean.
- `VERSION` is the release source of truth. The crate reads it with `include_str!`, `Cargo.toml` and `Cargo.lock` carry the same value, `exuno release` bumps the three together, and a test in `src/lib.rs` fails when the crate version and `VERSION` disagree.
- Templates embed from `lib/templates/` through `include_dir!`. The binary never looks up an engine directory at runtime.
- Dependencies: include_dir, serde, serde_json, sha2, signal-hook, thiserror, toml_edit (Codex `config.toml` key ownership); dev: assert_cmd, predicates, tempfile. Add a crate only for a concrete command need. No argument parser: every command reads its own options as its Bash `cmd_*` did, and `cli::Command` matches only the command word. A YAML parser is never added: `yaml_subset` reads the supported shapes by design.

## Structure

- `src/main.rs` is the only process-aware file: arguments, environment, `ExitCode`. Everything else is a library with a `Result<_, Error>` API.
- `src/cli/<cmd>.rs` (a `src/cli/<cmd>/` directory once it outgrows one file) owns one command through a public entry point named for it (`init`, `doctor`, `refresh`, …; `run` where the name would collide) that takes `out`/`err` writers and returns the exit status as `Result<u8, Error>` — or a bare `u8` where the module reports its own failures (`sync`, `rollback`). A pure `render(…) -> Result<String, Error>` beside `run` keeps the output testable when a command has one. Writes go only through those writers or the log sink `main` hands it; core modules never print.
- `src/error.rs` is the single error type. Each variant's `Display` text is the message the user sees, and `main` maps the variant to the exit code.
- Two output voices, kept apart: `style` for command modules; `log` for the engine. Neither leaks into the other's module.
- Engine paths are `/`-separated strings, drive-aware on Windows (`paths`). A disk path enters through `from_disk`/`DiskText`; arguments stay verbatim.
- Documented behaviour is the contract, quirks included; a deliberate change to it is one line under "Accepted deviations" in the spec, with its test.

## Verification

- `cargo test` is hermetic: `tempfile`, no network, no dependency on the developer's `~/.exuno`. A unit test asserts an observed value, never one derived from reading the code.
- A pure function's behaviour is a unit test beside it in `src/`; a command's observable contract — exit status, stream, files on disk — is an integration test in `tests/<surface>.rs`. A change to a command's output or exit status updates that file in the same commit.
- Output is stable across platforms when stdout is not a terminal; on a terminal the escape codes come from `style`.
