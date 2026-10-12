# MCP library roadmap

The problem from [PR #16](https://github.com/yelmuratoff/agent_sync/pull/16) is
worth keeping: a user should be able to find a documented MCP connection,
choose a variant, and prepare an Exuno source without copying server JSON
by hand. PR #16 targets the retired Bash engine and conflicts with the current
Rust code, so its implementation is a reference, not a branch to merge.

[PR #20](https://github.com/yelmuratoff/agent_sync/pull/20) delivers the first
slice. `mcp list/show/validate` reads a selected local catalog, validates v1/v2
manifests, and does not write configuration or contact an MCP server. The next
slice, `mcp render <id>[@variant]`, selects one connection and prints Exuno
MCP source JSON. Omitted suffixes select `default`; `@recommended` requires
attributed guidance. Rendering makes no configuration changes and does not
execute commands, probe URLs, install packages, or read secrets. The
[catalog contract](mcp-library.md) defines both read-only commands.

`mcp use <id>[@variant] --tool <slug>` previews a per-tool source and writes it
only with `--apply`. It leaves `exuno sync` as a separate action. Source
changes can be undone with `exuno rollback`.

## Delivered slices

1. **Guarded extension of an existing source.** `--merge` adds one selection to
   an occupied regular per-tool source. It preserves unrelated JSON members,
   rejects duplicate or unsupported structures, and requires an explicit ID
   for replacement. Replays, conflicts, backups, and lock contention have
   command-level coverage. Shared, legacy, and alternate sources are not
   migrated. A stale lock after interruption blocks later writes until the
   source is inspected and the lock removed.

2. **Client adapters.** Claude, OpenCode, and Kimi catalog selections have
   command-level coverage through their normal `sync`, `check`, `doctor`, and
   applicable `adopt` paths. Codex composes supported MCP fields into its
   settings file and refuses ownership conflicts. Cross-platform CI remains
   the compatibility gate for each adapter change.

The opt-in [pilot catalog](../catalog/mcp/README.md) contains Microsoft Learn,
Context7, and Octocode, checked against vendor or maintainer documentation on
2026-09-23. Its recommendations identify their sources and dates. Validation
proves catalog structure, not that a service is safe, reachable, or endorsed.

Each slice should be a separate reviewable PR with focused integration tests.
Source writes and client composition cross into `src/engine/render/` and
require their command-level tests as well.

## Not planned as part of this library

Automatic server installation or launch, OAuth or credential binding, endpoint
probing, implicit variant fallback, and live catalog-to-client synchronization
are outside this workflow. A catalog selection is an explicit snapshot; later
catalog edits must not silently alter a project's MCP configuration.
