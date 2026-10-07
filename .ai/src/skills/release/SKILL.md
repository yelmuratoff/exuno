---
name: release
description: Bump the Exuno version, write a CHANGELOG entry summarising changes since the last tag, and prepare a clean release commit. Use this skill when the user asks to release, ship, cut a version, bump major/minor/patch, tag, or prepare a new version — including phrasings like "let's ship 0.12", "bump the version", "make a release", or when asked to update CHANGELOG.md after a stretch of work.
---

# Release

Prepare a new Exuno release: run pre-release checks, bump VERSION, update CHANGELOG.md, commit.

## Pre-release checks

Run these **before** touching VERSION or CHANGELOG. If any check surfaces a problem, stop and report it to the user — don't silently auto-fix docs or skip a failure.

1. **Working tree clean** — `git status --porcelain` returns empty. Uncommitted work first, then release.
2. **Tests pass** — `cargo fmt --all --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, then `cargo build --release` succeed. Stop on failure; failing tests are never a "release later" problem.
3. **ShellCheck clean** — `shellcheck -x -S warning -e SC1091 install.sh lib/templates/guard/claude.sh` returns 0.
4. **CHANGELOG covers all user-facing commits since the last tag**:
   - `git log --oneline $(git describe --tags --abbrev=0)..HEAD` lists the candidates.
   - For each commit, decide: user-visible (CLI flag, output format, new tool, bug a user could hit) → must appear in the new CHANGELOG section. Internal refactor / test-only / CI / dev tooling → stays out.
   - If a user-visible commit is missing from your draft, add it before bumping.
5. **Docs reflect current behaviour** — spot-check that the release does not ship with stale docs:
   - `README.md` — command list, flag list, supported tools table. If a command/flag was renamed, removed, or added since the last tag, README must match.
   - `.ai/src/commands/*.md` and `.ai/src/skills/**/SKILL.md` — descriptions, argument hints, examples line up with what the binary actually accepts (`src/cli/usage.rs`, `src/cli/mod.rs`).
   - `.ai/src/rules/*.md` — invariants and module map (`architecture.md`) match the current file layout in `src/`.
   - `lib/templates/tools/_TEMPLATE.yaml` — every YAML option read by `src/engine/render/` and `src/config/tool.rs` is documented; no documented option is dead.
   - Cross-check `git log --since="<last-tag-date>" --name-only` against the doc files: if `src/cli/` or `src/engine/render/` changed and no doc file did, ask whether docs need a follow-up before tagging.

When a doc gap is real, fix it in the **same release commit** (or a separate commit immediately before) rather than punting to a "docs" release later — release notes that lie age the project faster than missing features.

## Steps

1. **Pre-release checks** — Run the five checks above. Stop on the first failure and report.
2. **Determine bump type** — `major` (breaking changes), `minor` (new features), `patch` (bug fixes). Default: `patch`.
3. **Calculate new version** — Parse `MAJOR.MINOR.PATCH` from VERSION, increment the appropriate component.
4. **Write CHANGELOG entry** — Add `## X.Y.Z` section at the top of `CHANGELOG.md` (after `# Changelog`):
   - Group under `### Added`, `### Changed`, `### Fixed`, `### Removed`.
   - Bold the feature/component name: `- **Export command:** added --dry-run support.`
   - Describe user-facing impact, not implementation details.
   - Match the tone and format of existing entries.
5. **Update the version** — Write the new number to `VERSION` (no `v` prefix, one trailing newline, as `exuno release` writes it) and the same value to `Cargo.toml` and `Cargo.lock`; the test in `src/lib.rs` fails when they disagree.
6. **Commit** — `git add VERSION Cargo.toml Cargo.lock CHANGELOG.md <any doc files fixed above> && git commit -m "release: vX.Y.Z"`.
7. **Leave the push to the user** — CI handles the rest:
   - `auto-tag.yaml` creates the annotated git tag when VERSION changes on `main`, using the CHANGELOG section as the tag message, and dispatches `release.yml`.
   - cargo-dist builds the platform archives and publishes the GitHub Release; `exuno update` downloads the binary from it.

## CHANGELOG Format

```markdown
## 0.5.0

### Added
- **Feature name:** Description of what was added and why it matters.

### Changed
- **Component:** What changed and how it affects users.

### Fixed
- **Bug description:** What was broken and how it's fixed now.
```

## Gotchas

- Write the VERSION file as `0.4.3`, never `v0.4.3` — the `v` prefix breaks the auto-tag workflow.
- Leave git tag creation to CI. `auto-tag.yaml` runs when VERSION changes on `main`.
- Stop after the commit — let the user review before pushing.
- CHANGELOG entries extracted by CI use `awk` with exact `## X.Y.Z` matching — the version header must be `## X.Y.Z` with no extra text.
- The `exuno release` CLI command exists for maintainer use with interactive confirmation. The AI workflow edits files and commits.
- Keep CHANGELOG entries to user-visible changes. Internal refactors with no behavioural effect stay out.
- Doc fixes uncovered by the pre-release checks ride **with** the release commit; never ship a known-stale README "to fix next time".
