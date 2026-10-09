---
paths:
  - "src/**"
  - "lib/**"
  - ".ai/src/tools/**"
---

# Architecture Rules

The sync engine is config-driven: shipped tool behaviour lives in `lib/templates/tools/*.yaml`, project overrides live in `.ai/src/tools/`, and generic engine modules in `src/` do the work. Extend by adding a YAML option, then handling it in a module — not by branching on a tool name.

## Config-Driven Sync Engine

- Declare shipped behavior in `lib/templates/tools/*.yaml` and repository-specific differences in `.ai/src/tools/*.yaml`. `render::run_passes` reads the resolved `targets.*` values of each `Tool` and routes to the generic copy, conversion, and composition modules.
- Express format differences through YAML target options (`extension`, `header`, `merge_to_file`, `inline_into_agents`, `prepend_agents`, `append_imports`, `as_skills`). Add a new option and a module path rather than an `if slug == "cursor"` branch.
- Resolve source paths through `overlay::Sources`: project `exuno.yaml` → global `config.yaml`. Read a tool field through `Tool::value`, which layers base, user, and profile YAML.
- Read defaults (`enabled`, `cleanup`) from `config.yaml` once, then propagate from there.

## Module Map

```
src/main.rs                    → process boundary: arguments, exit codes, environment; matches `cli::Command` and wires each command.
src/cli/<cmd>.rs               → one command each (`init/`, `doctor/`, `refresh/` are directories); `usage.rs` owns help and the unknown-command refusal.
src/config/                    → what a project and the engine declare:
  yaml_subset.rs / yaml_edit.rs                    supported YAML scalar, nesting, and list shapes; line-preserving edits.
  tool.rs / catalog.rs / payload.rs                layered base/user/profile config, embedded templates, payload resolution.
  project_config.rs / profiles.rs / version.rs     which `exuno.yaml` a project uses, config-home variants, the `version_pin` policy.
  snapshot.rs / template_manifest.rs / format_rev.rs  update diffs and the conflict queue, template content hashes, the project format revision.
  edit_paths.rs                                    where payload overrides are edited.
  mcp_catalog.rs                                   bounded, read-only MCP catalog manifests.
  skill_metadata.rs / skill_cards.rs / skill_source.rs  `SKILL.md` frontmatter, the skill-card TSV, the pinned Git blob behind a card.
  command_surfaces.rs                              what a source has a tool run: MCP commands, hooks, skill scripts.
src/engine/                    → the render:
  render/ / session.rs                             sync and check orchestration: prepare, per-tool passes, steps, checkpoint.
  overlay.rs / workspace.rs                        source overlays, the virtual file tree.
  file_ops.rs / staging.rs                         safe copying, directory sync, cleanup, write-then-rename.
  rules.rs / convert.rs / opencode_json.rs         rule headers and merges, target format conversion and composition.
  codex_toml.rs                                    MCP servers composed into Codex's `config.toml`.
  keyed.rs / toml_keys.rs / json_keys.rs           owned-key merges into a settings file another program writes.
  filters.rs / gitignore.rs                        include/exclude matching, the managed `.gitignore` block.
src/transaction/               → what makes a mutating run restorable:
  backup.rs / witness.rs / manifest.rs             transactions, post-operation witnesses, ownership, and drift.
  interrupt.rs                                     signal traps a transaction arms.
src/output/                    → log.rs / style.rs / prompts.rs / changelog.rs: engine log voice, command colours, terminal prompts, changelog rendering.
src/paths.rs / text.rs         → containment, drive-aware `/`-separated paths; byte-level line and whitespace handling.
src/zip.rs                     → ZIP read and deterministic write for `.skill` packages and `.zip` imports.
src/remote.rs                  → a `.ai/` fetched from a git remote: URL and web-link parsing, ref resolution, shallow sparse fetch.
src/project.rs                 → the project being operated on: its root and `exuno.yaml`.
src/error.rs                   → the single error type; `main` maps variants to messages and exit codes.
src/lib.rs                     → the library root: the group list and the test that pins the crate version to `VERSION`.
lib/templates/                 → shipped tool/payload bases and init/refresh content, embedded at build time.
```

Business logic lives in the library modules. `main.rs` stays a router, and a `src/cli/<cmd>.rs` module renders through the shared modules so `sync` and `check` stay aligned.

## Hard Constraints

- **Path safety**: `Paths::resolve_dest` rejects paths outside the canonical root. Resolve through `normalize` + `canonicalize_with_existing_ancestor` — these exist so the engine never needs `realpath` semantics on a partly missing path. `Paths::is_safe_source` allows the canonical project root, the virtual engine and overlay roots (`/<agentsync>`, `/<agentsync-overlay>` — the latter is what `overlay::setup_shared` builds for `shared:` and tears down at the end of the pass), and the explicit roots `render::prepare` registers through `register_explicit_roots`: a `source.*` directory outside the project, admitted only when `classify_explicit_source` finds it inside a directory `EXUNO_EXTERNAL_SOURCE_ROOTS` trusts (`trust_external_roots`) — an untrusted one stops the run, as does one naming the filesystem root, the home directory, or a project ancestor. `escaping_source_link` holds source symlinks to the same two sets. Engine paths are `/`-separated strings; disk paths enter through `paths::from_disk`.
- **Zero external deps**: the standard library plus the crates in `Cargo.toml`. The binary reaches its goals without `yq`, `jq`, `python`, `node`, `perl`, or `eval` — reads YAML through `yaml_subset`.
- **YAML parser scope**: scalar keys, dot-notation nesting, and the explicitly supported list forms. New YAML shapes need a concrete engine requirement and parser tests.
- **Idempotency**: `exuno sync` produces identical output on repeated runs. No timestamps, no ordering changes, no platform-dependent sorting. `exuno check` verifies this.
- **Stateless runs**: read config fresh each invocation. The version comes from the `VERSION` file through `include_str!`, never embedded by hand.
- **Transactional mutation**: `init`, `sync`, and `rollback` snapshot their complete managed write set through `backup::create`. Failures restore the previous state; operations prune completed history through `backup::prune` once no restore is pending.
- **Interrupts**: a transaction arms `interrupt` before its first write. A signal is recorded, the run stops at its next step and restores, then re-raises the signal; a staging file finished with a rename uses `staging` so the rename stays on one filesystem.
- **Document new inline options** (`inline_into_agents`, `prepend_agents`, etc.) in the `exuno` skill and `_TEMPLATE.yaml` as part of the change that introduces them.

## Data Flow

1. `render::prepare` resolves source overlays and the layered tool catalog; `render::run_passes` dispatches each target to the generic copy, conversion, composition, and manifest modules.
2. Modules operate on resolved inputs rather than tool identity; `render::checkpoint` records the witness, and a failed transaction restores before the command returns.
