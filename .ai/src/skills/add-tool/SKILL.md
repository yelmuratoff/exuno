---
name: add-tool
description: Add support for a new AI coding tool to Exuno's sync engine — create the tool YAML, configure targets and converters, add base templates, and write tests. Use this skill when integrating a new AI assistant, IDE, or editor into Exuno, when a user asks "can Exuno sync to X", or when extending the sync surface to a new agent — even if the user names the tool by brand without saying "tool" or "integration".
---

# Add New Tool Support

Add a new AI coding tool to Exuno so `exuno sync` distributes instructions to it.

## Steps

1. **Study current integrations** — Read `lib/templates/tools/claude.yaml` and the closest comparable tool config. Confirm the tool's current official project-level surfaces.
2. **Create the shipped tool YAML** — Copy `lib/templates/tools/_TEMPLATE.yaml` to `lib/templates/tools/<tool>.yaml`. Project-specific overrides belong under `.ai/src/tools/`; shipped integrations belong in `lib/templates/tools/`.
3. **Configure targets** — Define where each content type goes:
   - `agents` → main instructions file
   - `rules` → rules directory or merged file
   - `skills` → skills directory (if supported)
   - `commands` → commands directory (if supported)
   - `subagents` → agents directory (if supported)
   - `settings`, `mcp`, and `hooks` → tool-specific config or plugin files
   - `enabled: false` on categories the tool or a profile variant must not emit
4. **Handle format differences** — Check if the tool needs:
   - `.mdc` extension instead of `.md` (Cursor)
   - Frontmatter wrapping (`alwaysApply: true` for Cursor rules)
   - Inline skill/rule indexes in the agents file (`inline_into_agents: true`)
   - Commands rendered as skills (`as_skills: true`)
   - `00-context.md` pattern for AGENTS.md content in rules dir
   - TOML format for agents (Codex)
   - Safe Markdown/JSON composition for OpenCode-style shared files
5. **Extend generic conversion only when required** — Add reusable behavior in `src/engine/convert.rs`, `src/engine/rules.rs`, or a focused composition module, with unit tests. Keep `src/engine/render/` as orchestration and avoid tool-name branches.
6. **Add optional payload bases** — Put shipped settings, MCP, or hooks under the matching `lib/templates/<resource>/` directory only when the tool supports that surface.
7. **Update documentation** — Keep README support tables, the bundled Exuno skill, `lib/templates/tools/_TEMPLATE.yaml`, and CHANGELOG aligned with the new target.
8. **Write tests** — Add assertions in:
   - `tests/sync.rs` — verify output files exist
   - `tests/sync_options.rs` — verify `--only`/`--skip` filtering
   - `tests/check.rs` — verify `exuno check` detects drift
   - focused converter/composition tests when the tool changes formats
9. **Verify locally** — Run `cargo fmt --all --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` (or `cargo test --test sync` while iterating), `cargo build --release`, a repeated sync idempotency check, and `exuno check`. CI confirms all supported platforms.

## Gotchas

- Every tool has quirks. Read the tool's docs for where it expects instruction files.
- Some tools share output paths (e.g., Copilot uses `.github/`). Check for collisions with existing tools.
- Use only YAML shapes supported by `src/config/yaml_subset.rs`; include/exclude filters support scalar, inline-list, and block-list forms.
- The catalog discovers `lib/templates/tools/*.yaml`; do not add a command-local registration list.
- Tool names must be lowercase and match the YAML filename (e.g., `claude.yaml` → tool name `claude`).
- Credentials and global-only preferences stay outside project sync.
- Generated output may share a destination with another tool. Reserve namespaces and cleanup ownership explicitly so one target never sweeps another's files.
