# Experimental skill cards

`exuno skills catalog` displays skill metadata from an explicitly selected,
manually maintained catalog. Use it to inspect purpose, limitations, taxonomy
mappings, and declared requirements before deciding which skills to use.
The commands are read-only: they do not discover, select, install, or run skills.

```text
exuno skills catalog list --catalog FILE [--source ALIAS=LOCAL_REPO]... [--include GLOBS] [--exclude GLOBS]
exuno skills catalog show ID --catalog FILE [--source ALIAS=LOCAL_REPO]...
```

The catalog is an explicitly selected UTF-8 TSV regular file, limited to 128
KiB and 256 cards. Every row is checked before either command writes stdout.
The internal experimental format has exactly these ten fields:

```text
id  source  commit  path  oasf_version  oasf_terms  mapping  use_when  not_for  requirements
```

Fields are nonempty and tab-separated, with LF line endings. Control characters
are rejected. Use `-` for unknown OASF, purpose, limitation, or requirement
values, not empty fields; `mapping` still requires one of its named values.
`id` and `source` are lowercase slugs; `commit` is a
full 40- or 64-hex object ID; and `path` is a safe relative path (or `.`).
`oasf_version` is `vX.Y.Z`, or `-` when its unknown state is explicitly
documented. `oasf_terms` is `-` or a comma-separated list of taxonomy-shaped
paths. The CLI validates that spelling only; it does not assert taxonomy
membership. `mapping` is `clear`, `broad`, `partial`, or `unmapped`.

OASF is the [Open Agentic Schema Framework](https://github.com/agntcy/oasf).
The catalog format can record a pinned OASF taxonomy version and curator
mappings. The example leaves them unknown rather than claiming an unverified
mapping. These annotations do not extend the Agent Skills specification.

`--include` and `--exclude` filter card IDs with the existing Exuno glob
rules; exclusions win. They can be repeated. There is deliberately no `--set`
option and no set-based selection behavior.

## Source declarations versus verification

A catalog's `source`, `commit`, and `path` are declarations made by the
catalog author. Supplying `--source alias=LOCAL_REPO` lets Exuno inspect
only the matching pinned local Git object. It never checks out, fetches,
discovers, or contacts a source. A missing alias, unavailable repository,
missing object, unsupported metadata, or missing `SKILL.md` is shown as an
unavailable or unknown source state for that card; it does not remove the card
from `list`.

Git is therefore optional: the base commands have no extra runtime dependency,
and Git is needed only when a `--source` mapping is supplied. A mapped source
must be its repository root; ordinary worktrees and bare repositories are
supported. Tags are not accepted as pins: catalogs declare full commit IDs.
The pinned `SKILL.md` must be a regular UTF-8 Git blob of at most 128 KiB;
symlinks are not followed.

`show` keeps the distinction visible. `Source declaration` comes from the
catalog. `Source status`, `Frontmatter`, `Name`, and `Description` come from
the pinned object only when it is readable. An `available` source status proves
only that the object was read; it does not mean the frontmatter was supported
or that a name or description was available. The command never invents a
canonical name.

The frontmatter reader is intentionally a conservative subset, not a full YAML
validator. It reads ordinary scalar `name` and `description` fields and a
small block-string form (`>` or `|`, with an optional `+` or `-`). That form
also works for optional top-level fields, including arbitrary extension
fields, and for values in the one-level `metadata` mapping. Top-level block
content must use exactly two spaces; metadata block content exactly four; block
lines cannot be blank or more deeply indented. This lets common fields such as
`compatibility`, `metadata.source`, and `metadata.when_to_use` be folded or
literal strings without making the reader a full YAML parser. Other top-level
fields may use simple scalars under unquoted ASCII keys; `metadata` entries may
use the same simple scalar form. `allowed-tools` remains a string as defined
by Agent Skills: YAML lists are unsupported. Escaped double-quoted strings,
plain multiline scalars, deeper mappings or block content, YAML indicators/tags,
collections, malformed quotes, duplicate or missing required fields, and other
unsupported YAML anywhere in the frontmatter leave name and description
`unknown`; they are not silently interpreted.

## Curator notes are not runtime facts

`use_when`, `not_for`, `requirements`, OASF fields, and mapping describe the
curator's card. They are **UNVERIFIED** annotations, not a promise that a skill
is installed, portable, safe, authorized, or suitable for a workload. In
particular, requirements and capability-like descriptions are neither probed
nor provisioned. Requirements are free-text notes that may name binaries,
environment variables, or runtimes; they do not form an executable dependency
manifest. No card field is executed.

The one-card example is at
[`docs/examples/skill-cards/pilot/catalog.tsv`](examples/skill-cards/pilot/catalog.tsv).
It points to the [PDF skill at a pinned commit of Anthropic's public skills
repository](https://github.com/anthropics/skills/blob/34040c9c568585f6929bedeaad110ad08f079624/skills/pdf/SKILL.md).
The example demonstrates inspection, not endorsement or permission to use
that skill; its bundled license and dependencies require separate review.

```bash
exuno skills catalog list \
  --catalog docs/examples/skill-cards/pilot/catalog.tsv \
  --source anthropic-skills=/work/anthropic-skills

exuno skills catalog show pdf \
  --catalog docs/examples/skill-cards/pilot/catalog.tsv \
  --source anthropic-skills=/work/anthropic-skills
```
