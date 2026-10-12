# How sync works

`exuno sync` reads `.ai/src/`, works out what every enabled tool needs, and writes it. This page follows one run from start to finish, then explains where each tool's config comes from and how Exuno notices a hand-edited file.

## One run, step by step

1. **Read the project.** Exuno reads `.ai/exuno.yaml` for the enabled tools, the source paths, and the outputs mode. It accepts two source layouts: the structured one `init` creates (`.ai/src/AGENTS.md`, `.ai/src/rules/`, ...) and an older flat one (`.ai/AGENTS.md`, `.ai/rules/`, ...), which it detects on its own.
2. **Skip if nothing changed.** With `--if-stale`, Exuno compares the source files with the time of the last sync and exits at once when nothing is newer. The shell and git hooks use this.
3. **Refuse unsafe input.** A source symlink pointing outside the project, or a version pin that doesn't match the running binary, stops the run before anything is written. See [sources outside the project](configuration.md#sources-outside-the-project) and [version pinning](team-and-automation.md#pin-the-exuno-version).
4. **Add inherited content.** If the project inherits from a parent (`shared:`, see [Workspaces](workspaces-and-profiles.md)), Exuno fills the gaps in your source with the parent's files. It also adds the built-in `exuno` skill (see [Skills](skills.md#the-built-in-exuno-skill)). Both happen in memory; nothing is written into `.ai/src/`.
5. **Load every tool's config.** Each enabled tool's YAML is the shipped base with your override from `.ai/src/tools/` on top. See [where each setting comes from](#where-each-setting-comes-from) below.
6. **Check for hand edits.** Exuno compares every generated file with the hash it recorded last time. If one was edited, the run stops. See [catching hand edits](#catching-hand-edits).
7. **Lock and back up.** Exuno takes a project lock so a second run can't start, then copies everything it may change into `.ai/backups/`. See [Backups and rollback](backups-and-rollback.md).
8. **Write each tool's files.** For every tool, in this order:
   - `AGENTS.md` is copied under the tool's name (`CLAUDE.md`, `GEMINI.md`, or `00-context.md` inside the rules folder for Amazon Q).
   - Rules are copied with the tool's extension and header, merged into one file, or listed as short references in the agents file.
   - Skills are copied as folders, or listed as an index in the agents file.
   - Commands use the first mode the tool supports: a native folder, then TOML (`format: toml`), then generated skills (`as_skills`, written to `<skills dest>/command-<name>/SKILL.md`), then a `## Commands` index in the agents file.
   - Subagents are copied, renamed, or converted to the tool's format.
   - Settings, MCP, and hooks are resolved from your overrides and the shipped base.
   - A `post_sync` command runs only when `EXUNO_ALLOW_POST_SYNC=true` is set in the environment. The repository can't turn it on by itself.
9. **Finish.** Exuno updates its block in `.gitignore` and writes `.ai/.sync-manifest`. Files of tools you turned off are removed. Old backups are pruned.

Running `exuno sync` twice in a row gives the same files. `exuno check` relies on that: it renders everything in memory and compares the result with what's on disk.

## Where each setting comes from

Exuno doesn't copy every tool's config into your project. Each resource has a shipped base inside the binary, and you override only what you need:

```
1. Per-tool override           2. Shared MCP (MCP only)     3. Shipped base
   .ai/src/tools/<tool>/   →      .ai/src/mcp.json      →      built into the binary
   <resource>.<ext>                                              (lib/templates/<resource>/<tool>.<ext>)
```

For hooks, MCP, and settings, the first file that exists wins and is used whole. A tool YAML override works differently: see the note below the table.

| Resource | Shipped base | Per-tool override | Shared override |
| --- | --- | --- | --- |
| tool YAML | `lib/templates/tools/<tool>.yaml` | `.ai/src/tools/<tool>.yaml` | none |
| hooks | `lib/templates/hooks/<tool>.<ext>` | `.ai/src/tools/<tool>/hooks.<ext>` | none |
| MCP | `lib/templates/mcp/<tool>.json` | `.ai/src/tools/<tool>/mcp.json` | `.ai/src/mcp.json` |
| settings | `lib/templates/settings/<tool>.<ext>` | `.ai/src/tools/<tool>/settings.<ext>` | none |

A tool YAML override doesn't have to repeat the whole file. Exuno merges it field by field over the base.

Projects from 0.10 and earlier may still keep overrides in `.ai/src/hooks/<tool>.<ext>`, `.ai/src/mcp/<tool>.<ext>`, or `.ai/src/settings/<tool>.<ext>`. Those are still read and still beat the base, with a one-time deprecation warning. `exuno migrate --legacy` previews moving them and `exuno migrate --apply` moves them; see [Migrating](migrating.md#from-the-010-flat-layout).

What this means in practice:

- **A new project stays small.** `init` writes `.ai/exuno.yaml`, `AGENTS.md`, and the content folders you chose. It writes no hooks, MCP, or settings for tools you don't use.
- **Engine updates reach you.** After `exuno update`, the next sync uses the new base for every file you haven't overridden.
- **One MCP file serves every tool.** `.ai/src/mcp.json` reaches every enabled tool with an MCP target and is converted where the format differs. OpenCode gets it validated and merged into `opencode.json`; Codex gets it in `.codex/config.toml`.
- **MiniMax Code and Claude Code share `.mcp.json`.** If their MCP sources differ, sync stops before writing either. MiniMax may start a configured server while discovering tools, so review an MCP source before you sync it.
- **Two tools can't fight over one agents file.** If enabled tools would write different content to the same `AGENTS.md`, sync stops before writing either.
- **Settings files a tool also writes are shared.** When you sync into a config home (the folder a tool reads its user-wide config from, such as `~/.claude/`, either by running Exuno in `$HOME` or through a [profile](workspaces-and-profiles.md#profiles-separate-config-homes)), Exuno owns only the keys `.ai/src` declares in each TOML or JSON settings or MCP file (one entry per MCP server). It keeps what the tool wrote there itself, such as Codex's project trust and plugins, Claude Code's model, theme, and plugin choices, or a server added in Cursor. If the tool changed a key you declare, sync stops and names it; `exuno adopt <file>` pulls the change into the source. `targets.settings.ownership` and `targets.mcp.ownership` (`auto`, `keys`, `file`) override this. Inside a repository `auto` owns the whole file, and Zed's commented settings are always owned whole.
- **Overrides are per tool.** `exuno customize cursor hooks` copies the current base into `.ai/src/tools/cursor/hooks.json`. Delete that file later to go back to the base.
- **Hooks need someone to look first.** `customize <tool> hooks` prints the base content and asks for confirmation, or needs `--yes` without a terminal.
- **Stale copies can be cleaned up.** `exuno simplify` lists overrides that are still identical to the base, and `--apply` removes them so they stop pinning old defaults.

## Catching hand edits

After every successful sync Exuno writes `.ai/.sync-manifest`, with one line per generated file and its SHA-256 hash. On the next sync it checks each file:

- unchanged since the last sync: rewritten quietly
- deleted: rewritten quietly
- edited: the sync stops, lists the edited files, and leaves them alone

```text
[ERROR] Manual edits detected in 1 destination file(s) since last sync:
      .claude/rules/core.md

  These files would be silently overwritten. Choose one:
    • Move your edits into .ai/src/, then re-run sync
    • If a tool wrote here out of band, run 'exuno adopt <file>' to pull it into .ai/src/
    • Re-run with --force to discard the edits and rewrite from source
```

`exuno check` and `exuno doctor` report the same drift, so you see it before anything is lost. How to bring an edit back into the source is in [Editing a generated file](team-and-automation.md#editing-a-generated-file).

The manifest is tracked by git exactly like the files it describes: committed with them, or ignored with them. If the two differed, a teammate's `git pull` would look like a hand edit on the next sync.

## What a sync replaces

Generated folders belong to Exuno. Anything there that isn't in `.ai/src/` gets replaced or removed:

| Target | What happens |
| --- | --- |
| Rules folders (`.claude/rules/`, `.cursor/rules/`, ...) | Files with the synced extension are managed. Ones without a source are removed. |
| `AGENTS.md`, `CLAUDE.md`, `GEMINI.md`, ... | Replaced from `.ai/src/AGENTS.md` on every sync. |
| `settings.json`, `.mcp.json`, `hooks.json` | Replaced from their source, except the shared settings files described above. |
| Skills, commands, and agents folders | Replaced from the source. Extra files are removed. |
| `.gitignore` | Only the block between `AI SYNC GENERATED START` and `END` is managed. |

If you already have tool config, `exuno init` adopts it into `.ai/src/` first. To move files by hand, see [Migrating existing tool configs](migrating.md#existing-tool-configs).

## Words used in these docs

| Word | Meaning |
| --- | --- |
| source | What you edit: `.ai/src/` and `.ai/exuno.yaml` |
| generated files, outputs | What sync writes for each tool: `CLAUDE.md`, `.cursor/rules/`, and so on |
| tool YAML | The file that says where one tool's files go and in what format |
| base | The shipped default for a tool YAML, settings, hooks, or MCP file, built into the binary |
| override | Your file in `.ai/src/tools/` that replaces or changes a base |
| manifest | `.ai/.sync-manifest`, the list of generated files and their hashes |
| drift | A generated file that changed since the last sync, usually by hand |
| adopt | Copy existing content into `.ai/src/`: `init` does it for config a project already has, `exuno adopt <file>` for one edited generated file, `profile add --adopt` for an existing config home |
| config home | The folder a tool reads its user-wide config from, such as `~/.claude/` |
| profile | A second config home for the same tool, such as `~/.claude-hub/` for a work account |
