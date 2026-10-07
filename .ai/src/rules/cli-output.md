---
paths:
  - "src/**"
---

# CLI Output Rules

What the binary prints, and why. No standards body governs terminal output, so
each rule below says whether it follows a formal standard, a published informal
one, or a decision this project made.

## Streams

- Everything written for a person goes to stderr; stdout carries only what a
  program reads. POSIX reserves stderr for diagnostics, and `clig.dev` puts
  progress and status there too so stdout stays a clean pipe: `cargo`, `git`,
  and `npm` all do this. The whole sync log — `[INFO]`, steps, `[WARNING]`,
  `[ERROR]`, `[DONE]`, the `--workspace` banner — is on stderr, so `sync
  > out.json` and `sync | tee` behave. Stdout has `--help` and the `--json`
  summary, nothing else.
- A checker's report is its output. `check` printing drift on stdout is
  correct; its exit status carries the verdict.
- A hint that follows an error follows it on the same stream, the way rustc
  writes `error:` then `help:`. Never split an error from its hint.

## Exit codes

- Zero for success, non-zero otherwise, which is the only formal rule (POSIX).
- `check` follows grep's shape: zero when synced, one when out of sync. Keep
  errors distinguishable from a clean negative answer.
- Stay below 125: the shell reserves 126, 127 and everything above 128.
- `sysexits.h` is deprecated by its own manual page. Never adopt its numbers.

## Colour

- Colour when the stream being written is a terminal and `NO_COLOR` is unset
  or empty, as `no-color.org` specifies: the sync log asks stderr, a command's
  report asks stdout. `sync 2>log.txt` must leave no escape codes in the file.
  That is an informal standard with wide adoption, not a formal one.
- Colour is emphasis, never information. A line must read the same in a pipe.
- The GNU Coding Standards argue against looking at the terminal at all. Every
  modern tool ignores that, and so do we, deliberately.

## Glyphs

**No emoji.** Not a standard: `clig.dev` permits them. It is this project's
decision, on three grounds. Of the tools worth imitating — cargo, git, ripgrep,
kubectl, docker, npm, terraform, gh — not one prints emoji by default, and
cargo's status lines are pure ASCII. Emoji with the presentation selector are
Wide under Unicode Annex #11, which warns that the property "is not intended
for use by modern terminal emulators without appropriate tailoring", so one
glyph misaligns every column to its right. And a screen reader has no text
alternative for a bare glyph, the principle behind WCAG technique H86.

- The level tag carries the level: `[INFO]`, `[WARNING]`, `[ERROR]`, and
  `[DONE]` for the one closing line. Colour decorates the tag; the words
  survive a pipe. A tool's block has no closing tag of its own: its steps are
  indented under `[INFO] Syncing <tool>`, and a blank line ends it.
- Report markers are `✓`, `✗`, `!` and `·`. `→` separates a source from a
  destination. `•` opens a hint. None is an emoji and none needs a
  presentation selector.
- Never let a glyph be the only carrier of meaning. `✓ passed`, not a bare `✓`.
- `list` marks its table with `●`, `○` and `★`, and its legend spells each one
  out. Those three sit in Unicode Annex #11's Ambiguous width class, the same
  class this rule cites against emoji, so a terminal that renders them wide
  misaligns the columns after them. They stay for now because the legend
  carries the meaning in words and changing a command's main table is its own
  change, not a style sweep. Do not add another Ambiguous-width glyph.
- No rules between sections. A fixed-width line of `═` or `─` wraps in a
  narrow window and turns to noise in a log; a blank line separates blocks
  instead, as cargo's output does.
- A count reads as prose: `(8 updated)`, `(1 agent, md→toml)`. Zero of
  something is not printed, and a count of one is singular.

## Messages

- An error names what failed and what to do next. The GNU shape is
  `program: message`, lower case, no full stop; a hint belongs on its own line,
  the way rustc writes `error:` then `help:`.
- Say the command to run, not the concept: `run exuno sync`, never
  "re-synchronise the project".
- Never print an engine-internal path. The virtual roots (`/<agentsync>/…`,
  `/<agentsync-overlay>/…`) mean nothing to a reader: `Paths::display` names
  an overlay entry by its category (`rules/`, `AGENTS.md`) and a shipped file
  by its template path (`templates/guard/claude.sh`).
- A heading names what is happening, without a trailing `...`: `Syncing Claude
  Code`. The ellipsis promises a wait, and the next line arrives at once.
- The closing line carries the count; a list of what was skipped is dry-run
  detail. Eleven tool names on one line wrap in a narrow window and say
  nothing a second run needs.

## Quiet and machine-readable output

- `sync --quiet` keeps warnings, errors, and the closing `[DONE]` line.
- `sync --json` prints one object on stdout after a successful run — `dry_run`,
  `synced`, `total`, `skipped`, `written`, `preserved`, `backup` — and that
  object is the contract for scripts. The human log on stderr is not: it may
  change between releases, and `CHANGELOG.md` says so when it does.
- A field is added to the JSON object, never renamed or removed, without a
  major version.
