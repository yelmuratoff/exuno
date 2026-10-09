# Changelog

## 0.46.0

### Breaking

- **Import:** a source that brings MCP servers, tool config, or skill scripts lists them under `Runs commands:` and asks before writing them; without a terminal, pass `--force`.
- **Import:** keeps the project's own `exuno.yaml`; pass `--config` to take the source's.

### Added

- **Import from git:** any git URL, a GitHub or GitLab link to a ref and folder, or `owner/repo`; `--ref` and `--path` pick them, and private repositories use your git credentials.
- **Skill packages:** `import` reads `.skill` and `.zip` archives and folders of skills, and `export --skill <name>` writes a `.skill` for Claude's skill upload.
- **Export:** `-o bundle.zip` writes a ZIP.
- **Import:** an import that changes files is backed up first, so `exuno rollback` undoes it.

### Changed

- **Import:** an imported skill replaces the project's copy whole, so files its new version dropped are removed.
- **Import:** also reads `.tar`, `.tar.xz`, and `.tar.bz2`, and no longer needs `curl`.

### Fixed

- **Export:** the bundle now carries all of `.ai/src`, `mcp.json` included, and leaves hidden files and symbolic links out.
- **Import:** a symbolic link in an archive or a remote no longer brings in files from outside it.
- **Update and import:** their temporary folder is created private, so another user on a shared `/tmp` cannot tamper with it.

## 0.45.2

### Fixed

- **Migrate:** run as `agentsync`, suggests `agentsync migrate --apply` and `agentsync sync` rather than an `exuno` command that is not installed yet.
- **Setup-hooks:** with `core.hooksPath` set elsewhere, still rewrites an agentsync block in the repository's own hooks, so the `exuno setup-hooks` that `doctor` suggests for it works.

## 0.45.1

### Changed

- **Migrate:** moving the config to `.ai/exuno.yaml` now warns that releases before 0.45.0 cannot read it, so the team updates before the move is committed.

## 0.45.0

### Breaking

- **Rename:** AgentSync is now Exuno — the `exuno` command, the `yelmuratoff/exuno` repository, and installs in `~/.exuno`; the `agentsync` command, `AGENTSYNC_*` variables, `.ai/agent_sync.yaml`, `agentsync_version`, and `metadata.agentsync-*` keys keep working until 1.0.
- **New projects:** `init` and `enable` write `.ai/exuno.yaml` with `exuno_version`, which AgentSync 0.44 and earlier cannot read.
- **Project format r3:** every command reminds a project below r3, and `exuno migrate --apply` renames what still carries the old name and re-pins a project, and its CI gate, pinned before 0.45.0.
- **Bundled skill:** ships as `exuno`; a project copy under `skills/agentsync/` stops replacing it until `exuno migrate` moves it.

### Added

- **Doctor:** names every file, key, skill, and git hook that still carries the agentsync name.
- **Install:** links both `exuno` and `agentsync`, and `EXUNO_VERSION` pins a release from before the rename too.
- **Update:** installs a release from before the rename when a project pins one.

### Changed

- **Git hooks and shell-init:** call `exuno`, or `agentsync` when only that is installed, and `setup-hooks` replaces an older agentsync block instead of adding a second.
- **CI gate:** `init --ci github` writes `exuno-check.yml` and keeps an existing `agentsync-check.yml`.
- **Export:** writes `exuno-bundle.tar.gz` by default.
- **Releases:** keep publishing `agentsync-<target>` archives until 1.0, so `agentsync update` on an older install reaches them.

### Fixed

- **Codex:** a rule index synced from the home directory points at `~/.ai/src/rules`.
- **Bundled skill and command template:** no longer run a documented shell example or blank `$ARGUMENTS` when Claude Code loads them.

## 0.44.2

### Added

- **Update:** after a successful update, points to `agentsync refresh` for the project's rule, skill, and command templates.
- **Doctor:** advises when a project copy replaces the bundled `agentsync` skill and so stops receiving its updates.

## 0.44.1

### Fixed

- **Profiles:** a profile with its own `src/` now gets the bundled `agentsync` skill and its extensions, like the personal tools.

## 0.44.0

### Added

- **Extend an inherited skill instead of replacing it.** A directory named after the bundled `agentsync` skill, or after a skill a `shared:` parent provides, that has no `SKILL.md` of its own now adds to that skill: its files join the inherited ones, a file at the same path replaces the inherited one, and a `SKILL.append.md` is appended to the inherited `SKILL.md` at sync. The directory can sit at the skills root or inside any category. The inherited text keeps updating with the engine or the parent; a directory with its own `SKILL.md` still replaces the skill outright. `doctor` no longer reports such a directory as an empty skill.

### Changed

- **The bundled `agentsync` skill loads less up front.** Its `SKILL.md` is about half as long and points to topic references the agent reads when the task needs them: `maintenance.md` (now also backups, rollback, version pins, and the other commands), `workspaces-and-profiles.md`, `mcp-and-tool-targets.md` (with per-host notes on where an MCP server is actually live), `settings-hooks-and-harness.md`, `writing-skills.md`, and `evaluating-skills.md`, which is new. The writing guidance adds the removal and addition tests for rule lines, failure-mode vocabulary for skills, and a loop for turning an agent mistake into a harness change.

### Fixed

- **`update` prints code in the changelog as written.** The release notes it shows dropped every `**` along with the bold markers, so a glob such as `src/**` read as `src/`; text inside backticks now keeps every character. A nested bullet also wraps under its own text instead of starting its continuation lines to the left of it.
- **`migrate` finds an engine-owned skill copy inside a category.** A copy of the bundled `agentsync` skill moved to `.ai/src/skills/<category>/agentsync/` still overrides the one the engine ships, but `migrate` looked only at `.ai/src/skills/agentsync/` and reported no copy. It now finds the copy by name, removes it when unedited along with any category it leaves empty, and keeps it when edited.
- **A command's folded description reaches Codex, Kimi Code, Kiro, Amazon Q, and Zed whole.** A command whose `description:` is a `>` or `|` block became a `command-*` skill described only as `>`, and an entry in the inlined `## Commands` index reading `— >`; both now carry the folded text.

## 0.43.0

### Breaking

- **Windsurf output moves to `.devin/`.** Windsurf is now Devin Desktop and reads its project rules, skills, workflows, MCP config, and hooks from `.devin/`. The first sync after the upgrade writes there and removes the files it had recorded under `.windsurf/`; files you wrote there yourself stay. `list` shows the tool as Devin Desktop; its slug is still `windsurf`.
- **Cline gets native skills and its current layout.** Cline reads `AGENTS.md`, `.cline/rules/`, `.cline/skills/`, and `.cline/workflows/`, so skills are copied as skills instead of inlined as an index in `.clinerules/00-context.md`. The first sync removes the files it had recorded under `.clinerules/`.
- **The config reader no longer borrows a list or a section from elsewhere in the file.** An empty key such as `enabled:` followed by another key read the next `- item` list anywhere below it, so `tools.enabled` could pick up an unrelated list; it now reads as empty. A key holding a value, such as `version_pin: warn`, no longer hides a later mapping with the same name, so `version_pin:` with `mode: strict` further down now applies. A config written either way reads differently after the upgrade.
- **A tool override can set a field to empty.** `header: ""` or `exclude: []` in `.ai/src/tools/<tool>.yaml` used to fall back to the shipped value; a key you write now wins even when empty. A `base:` in an override of a shipped tool now fills the fields the shipped file leaves empty, where it used to be ignored.
- **Frontmatter fields read the first occurrence and whole quoted values.** A field written twice in a command took its last value, and `description: "use # with care"` was cut at the `#`; the generated `command-*` skills and the inlined command index now carry the first value, quoted text intact.
- **A rule's globs come only from `paths:`.** A rule with `paths:` also took every other list in its frontmatter, such as `tags:`, into the globs of Cursor's `.mdc`, Copilot's `applyTo`, and the other scoped headers. `paths: ["src/**"]` and `paths: src/**` now scope a rule too, where they used to leave it always on.
- **The inlined skill index shows a folded description whole.** A skill whose `description:` is a `>-` block was indexed as `-`, and a `>` block showed only its first line; the index now carries the description `skills show` prints.
- **`enable` adds `enabled:` inside an existing `tools:` block.** A config whose `tools:` had other keys but no `enabled:` got a second `tools:` block at the end of the file, which the reader then ignored.
- **A nested config key matches only at its own level.** `tools.enabled` also matched an `enabled:` nested deeper, such as `tools.foo.enabled`, when reading or editing a config; a key one level further in no longer answers for it.
- **`simplify --apply` deletes nothing off a terminal without `-y`.** A script running it removed byte-identical payload copies while keeping the override file it had emptied; both now wait for `-y`, and each kept file says so. Scripts that relied on the deletion add `-y`.
- **`migrate` leaves a file without an extension where it is.** A `README` under `.ai/src/settings/` moved to `.ai/src/tools/README/settings.README`, as if it were a tool's payload; it now stays, and `update` no longer counts it as a legacy override.
- **`refresh` off a terminal changes nothing without `--yes`.** When only auto-updates were pending it applied them silently, and with `--include-deleted` it printed restore prompts it then declined; any pending change now stops with the same `Use --yes` error new files and conflicts gave. CI jobs that ran `refresh` to take auto-updates add `--yes`.
- **`init` records no template hash for the files it adopts.** The template manifest was written before adoption, so an adopted `AGENTS.md` or rule carried the shipped template's hash and `refresh` treated the project's own text as an edit of the template. A project initialised earlier keeps the manifest it has.
- **`add mcp` keeps the rest of `.ai/src/mcp.json`.** Adding a server dropped every top-level key other than `mcpServers`, replaced a file without `mcpServers` wholesale, took an `mcpServers` nested inside another key for the real one, and silently lost servers after an invalid key. Other keys now stay in order, `mcpServers` is added beside them, and a file that is not valid JSON is refused and left as it is.
- **`import` follows the source project's `source.*` paths.** A project keeping, say, its commands in `custom/cmds` exported them, but no import read them back, and the imported `agent_sync.yaml` then pointed at a `custom/cmds` the new project did not have. Import now reads each section from its declared path, writes it into the project's own `.ai/src/`, and drops the `source.*` entries that would point elsewhere.

### Added

- **Skills can be grouped into category directories.** A directory under `.ai/src/skills/` without its own `SKILL.md` is a category, so `skills/flutter/bloc/SKILL.md` and `skills/backend/auth/SKILL.md` can sit side by side, up to four category levels deep. Claude Code, VS Code Copilot, and Gemini CLI skip nested skills, so `sync` lands every skill flat at `<dest>/<name>/` for every tool; the category exists only in the source. Skill names stay unique across categories: two skills with the same name stop the sync before anything is written, and the error names both.
  - `targets.skills.include` and `.exclude` take a category path as well as a name: `exclude: cloudflare/*`.
  - `agentsync add skill <name> --category flutter/ui` scaffolds into a category. `skills list` gains a category column, `skills show` a `Category:` line, and an inlined skill index groups its entries under `### <category>` headings.
  - `adopt` returns an edited skill to its category, and `doctor` reports a name collision, a skill nested deeper than four categories, an empty category, and a category name that is not lowercase-kebab.
- **Kiro.** `kiro.yaml` writes `AGENTS.md`, rules to `.kiro/steering/` with `inclusion: always` (or `inclusion: fileMatch` for a rule with `paths:`), skills to `.kiro/skills/`, commands as `command-*` skills, subagents to `.kiro/agents/` as Kiro Markdown agents, and MCP servers to `.kiro/settings/mcp.json`. Kiro's IDE and CLI succeed the Amazon Q Developer CLI; `amazonq.yaml` stays for projects still on Amazon Q Developer.
- **Three tool YAML options** for tool authors: `{globs_list}` in `scoped_header` becomes a quoted YAML list of a rule's globs, `format: kiro_md` converts a subagent to a Kiro agent, and `legacy_dest` names where an earlier version wrote a target so the move cleans up after itself.

### Fixed

- **`doctor` reports a secret beside an angle-bracket placeholder.** A line such as `host: <your-host> token: ghp_…` was skipped whole, so the token went unreported. Only the `<…>` span is skipped now, as `${…}` already was.
- **`simplify` finds payload overrides under `source.tools`.** With the tool override directory moved, it looked in `.ai/src/tools` and reported no payloads to remove.
- **`import https://github.com/user/repo.git/` downloads `repo`.** The trailing `/` hid the `.git` suffix, so the download asked for a repository named `repo.git`.
- **`profile add --tools` trims names and refuses an unknown tool.** `--tools 'claude, nope'` wrote a variant named ` nope-hub`, space included, for a tool that does not exist; it now stops with `unknown tool: nope.` and exit status 2. A `--tools` with no value says so instead of exiting 1 in silence.
- **`diff <slug>` no longer answers "No user overrides" for any slug.** In a project without overrides, a mistyped slug read as nothing to diff; it now fails with `No override found for '<slug>'`, as it already did when other tools had overrides.
- **`show <slug> <resource>` labels an override by the file sync reads.** An override such as `.ai/src/tools/cursor/hooks.jsonc` was shown as `[base]` because its extension differed from the shipped template's.
- **`disable` lists only the tools it disabled.** An unknown slug, and a slug named twice, appeared under `Disabled N tool(s)` beside the tools that were really switched off.
- **`sync --workspace` exits with the highest status of its projects,** as its `max exit code` line says. It exited with the last failure's status, so an OpenCode composition error (26) followed by an ordinary failure (1) ended as 1.
- **`adopt .rules` explains that Zed's rules file is merged.** It answered that `.rules` was not a recognised AgentSync output; it now says the tool merges rules into one file and names the source rules directory to edit.
- **A file in the way of an output directory is named.** `init` or `sync` with, say, a file where a tool writes a directory stopped with `Backup target parent is not a directory: <target>`; the message now names the file and says to move or rename it. A single-file `.clinerules` no longer triggers it, since Cline writes to `.cline/`.
- **`dedupe` keeps `.ai/src/rules/` and the other top-level source directories.** Deleting the last duplicate in one removed the directory itself; now only the empty folders below it go, such as a skill folder or a skill category left empty.
- **`init --tools` and `--content` read what was typed.** A space inside a name was dropped, so `cla ude` enabled `claude`; it is now refused. An empty value such as `--tools=` or `--content ,` skipped the wizard and contributed nothing, and `--content ,` scaffolded a project without `AGENTS.md`; both now stop with `requires a value`.
- **`add mcp` checks `--env` before creating `.ai/src/mcp.json`,** so a malformed pair no longer leaves an empty server map behind. `--args` and `--env` values that span lines are read whole: a newline separates arguments, and env pairs, like a space or a comma does.
- **`profile remove` deletes only what AgentSync generated in the config home.** It removed the whole directory, such as `.claude-hub/`, with the credentials, history, and settings the tool kept there. Files the sync manifest does not record now stay, and the command says `kept .claude-hub/` when any do.
- **`enable` and `disable` edit the config `AGENTSYNC_CONFIG_PATH` selects.** They wrote to `.ai/agent_sync.yaml` while every other command read the selected file, so a tool enabled there never synced. `disable` also no longer creates `.ai/agent_sync.yaml` in a project that has none; it switches the tool off in its own YAML only.
- **`upgrade-config` pins the config `AGENTSYNC_CONFIG_PATH` selects,** not `.ai/agent_sync.yaml`; a variable naming a missing file now stops it with the same error the other commands give.
- **`dedupe` reads `shared.path` from, and records declined templates in, the config `AGENTSYNC_CONFIG_PATH` selects,** resolved for each project under `--workspace`. A variable naming a missing file stops it.
- **`refresh` reads `template_overrides` from the config `AGENTSYNC_CONFIG_PATH` selects,** so templates declined or pinned there stay declined or pinned. A variable naming a missing file stops it.
- **`check` and `doctor` name the file a disabled target left behind.** With `targets.agents.enabled: false`, an earlier `CLAUDE.md` stayed on disk and in the manifest, nothing updated it, and `check` reported the project as synced. Both commands now say `CLAUDE.md is left from claude targets.agents, which is disabled` with the way out; neither changes its exit status, and sync still leaves the file alone.
- **`list` columns line up on a colour terminal.** Padding counted the escape bytes of a coloured cell, so every column after one drifted.
- **`diff <slug> <resource>` refuses an unknown resource before it looks for the project,** as `show` and `customize` do.
- **Removing a key keeps the blank line that separated its neighbours.** `simplify --apply`, `resolve` and `profile remove` joined the key before a removed block to the one after it.
- **`profile add` writes the profile's tools as `[a, b]`,** with the space every other flow list in the config has.
- **`adopt --all` names each adopted source once.** The same edit in two tools' copies of a skill printed `✓ adopted` twice for one file and counted it twice in the total.
- **`migrate --apply --yes` separates `removed .agent/` from the planned moves** with a blank line, as every other block of its report is.
- **`generate` keeps a description ended with Ctrl-D.** Closing the input while describing the project exited 1 and dropped what was typed; it now ends the description and prints the prompt. Closing it at the menu says `Cancelled.` instead of exiting in silence.
- **`setup-hooks --help` answers wherever `--help` appears,** as the other commands do.
- **`release` explains an input that ends at its prompt:** `input ended before an answer; nothing was released.` It exited 1 in silence.
- **`init` no longer writes `defaults.enabled`,** a key nothing ever read, so a new config does not suggest tools can be switched on there. An existing one keeps working; the key stays ignored. `defaults.cleanup` is read and stays.
- **`targets.commands.include` and `.exclude` now filter a native or TOML commands directory,** as `_TEMPLATE.yaml` documents. They applied only to generated `command-*` skills and the inlined command index, so an excluded command still reached Claude Code, Cursor, Gemini CLI, and the other tools with a commands directory.
- **`doctor` no longer suggests `agentsync simplify` for an empty skill directory.** `simplify` never touched skill directories; the advisory still says to populate or remove it.

## 0.42.0

### Added

- **Key ownership covers every settings and MCP file in a config home, not only Codex.** Claude Code writes the model, theme, effort level, and plugin choices into `~/.claude/settings.json`, and an editor such as Cursor adds servers to its global `mcp.json`, so a global sync stopped on those edits the same way it did on Codex's. Synced from `$HOME` or into a profile, AgentSync now owns only the declared keys of each TOML or JSON `settings` and `mcp` file: Claude Code's `settings.json` and `.mcp.json`, Gemini CLI's `settings.json`, the MCP files of Cursor, Windsurf, Junie, Amazon Q, Kimi Code, and MiniMax Code, and OpenCode's composed `opencode.json`. A plugin Claude Code enabled or a server Cursor added stays where the tool put it; a declared key the tool changed stops the sync by name, and `agentsync adopt <file>` moves it into the source. Every server in `mcpServers`, `mcp`, `mcp_servers`, or `context_servers` is owned as one entry; a changed one is named by the sync, and for a server that came from an MCP source `adopt` sends you to that source rather than splitting a shared `mcp.json` between tools.
  - `targets.mcp.ownership` joins `targets.settings.ownership`; both default to `auto` for every tool, which in a repository keeps owning files whole, so project output does not change. Codex and OpenCode compose one file from both, so `targets.settings.ownership` decides for it. Tools that write the same MCP file, such as Claude Code and MiniMax Code with `.mcp.json`, must use the same mode, or the sync stops before writing.
  - A JSON file is left byte for byte while its declared values match, and `60` matches `60.0`. When a declared value changes, the file is rewritten pretty-printed with its keys sorted, and `adopt` writes a JSON source the same way. JSON with comments stops the merge with a hint to use `ownership: file`; Zed's settings allow comments, so Zed's shipped tool owns its settings whole. A parse error says whether the source or the live file is at fault.
  - A tool you disable keeps its key-owned files, and they no longer hold up later syncs.
  - If you adopted a whole settings file before, it may carry state the tool wrote, such as Claude Code's `feedbackSurveyState`. Now that sync owns every declared key, remove such keys from `.ai/src`, or each change the tool makes to them stops the sync.

### Changed

- **New lines in the sync log.** A key-owned file is written as `<source> → <dest> (owned keys only)`, a disabled tool's key-owned file as `Kept <dest> (the app writes to it too)`, and a dry run warns `A real sync would stop: …` where the real run would stop on a whole-file switch or a first-run difference. The `--json` summary is unchanged.

## 0.41.0

### Added

- **The Codex app and AgentSync share `~/.codex/config.toml`.** The Codex app writes project trust, hook trust hashes, plugins, marketplaces, desktop preferences, and its own MCP servers (`node_repl`, `computer-use`) into the same file AgentSync generated, so every app update stopped the next global sync with `Manual edits detected`, and adopting the file copied that state into `.ai/src`. Synced from `$HOME` or into a profile, AgentSync now owns only the keys it declares: the leaves of the settings source and one `mcp_servers.<id>` per server in the MCP source. It keeps every other key, comments included, and removes a key only after it declared the key and then stopped. `.ai/src/mcp.json` becomes the one place for MCP servers, and the settings source holds only your own configuration.
  - A declared key the app changed stops the sync and is named: `.codex/config.toml (model)`. `agentsync adopt .codex/config.toml` copies only those keys into the settings source and keeps its comments; a changed `mcp_servers` entry is refused, because the MCP source owns it. `--force` rewrites the declared keys and nothing else.
  - The first sync in this mode removes nothing. It stops when a declared value differs from the live file, so a model you picked in the app is not replaced silently; `--force` applies the source.
  - `check` and `doctor` measure only the declared keys, and `rollback` restores the whole file.
  - `targets.settings.ownership` in `.ai/src/tools/codex.yaml` picks the mode: `auto` (the default: keys in `$HOME` and profiles, the whole file in a repository), `keys`, or `file`. Any other value stops the sync. On Windows without `HOME`, `%USERPROFILE%` counts as the home directory.
  - Nothing drops the app's keys behind your back: disabling Codex keeps the file (`Kept .codex/config.toml (the app writes to it too)`), `targets.settings.enabled: false` merges only the MCP servers, and switching a key-owned file back to `ownership: file` stops until you run `agentsync sync --force`. `adopt` refuses a whole-file copy of a key-owned file, and it will not create a missing settings source from a few changed keys; run `agentsync customize codex settings` first.

### Changed

- **`.ai/.sync-manifest` gains a third column** on the line of a file AgentSync owns by key: the declared keys with a short hash of each value, and the line's hash covers only those keys. A two-column line reads as before. An older `agentsync` reading the new line sees a hash mismatch and stops with `Manual edits detected` instead of overwriting.

### Fixed

- **`skills catalog --source` read folded and literal strings only in `name` and `description`.** A `SKILL.md` that wrote `compatibility`, an extension field, or a `metadata` value such as `metadata.when_to_use` as a `>` or `|` block showed its name and description as `unknown`. Those fields accept the block form now: two-space indentation at the top level, four under `metadata`.

## 0.40.1

### Added

- **Codex MCP servers take Codex's own fields.** Besides `command`, `args`, `env`, and `url`, a server composed into `.codex/config.toml` can carry `cwd`, `env_vars`, `enabled`, `required`, `startup_timeout_sec`, `tool_timeout_sec`, `enabled_tools`, and `disabled_tools`, and an HTTP server `bearer_token_env_var`, `http_headers`, and `env_http_headers`. A server that needed one of them, such as the Codex app's `node_repl` with `startup_timeout_sec`, can move out of the settings file into a per-tool `.ai/src/tools/codex/mcp.json`. A field Codex does not take is still refused, and the error now names it: ``MCP server docs: field `headers` is not supported for a Codex http server``.

### Fixed

- **`doctor` reported an MCP ownership conflict that `sync` does not have.** With `targets.mcp.enabled: false` in `.ai/src/tools/codex.yaml` or `opencode.yaml`, `sync` copies the settings file and leaves the MCP source alone, but `doctor` still failed with `Codex MCP ownership conflict` (or the OpenCode one). It skips the check when the MCP target is off.
- **The Codex ownership error says how to get out of it.** Under `Cannot compose Codex config: settings already contain or may encode mcp_servers`, `sync` prints the two ways out: move the `[mcp_servers]` tables into the MCP source, or set `targets.mcp.enabled: false` to keep them in settings. `doctor` names both too.

## 0.40.0

### Breaking

- **Codex receives the project's MCP servers, and its settings can no longer define their own.** `sync` appends the servers from `.ai/src/mcp.json`, or from a per-tool `.ai/src/tools/codex/mcp.json`, to `.codex/config.toml` after your Codex settings. Once either MCP source exists, even as `{"mcpServers": {}}`, a Codex settings file that still defines `[mcp_servers]` (the old shipped template suggested it) fails the sync with `Cannot compose Codex config: settings already contain or may encode mcp_servers`, and `doctor` reports the conflict. Move those servers into the MCP source and delete them from the settings file. Codex can take a server with `command`, string `args`, and string `env`, or one with an HTTP(S) `url`; a server with any other field, `headers` for one, fails the sync the same way. `adopt` refuses the composed `config.toml`, so edit the two sources separately.

### Added

- **`agentsync skills list|show|check`.** `list` reads the effective `source.skills` tree, shared and bundled skills included, with `--profile`, `--include`, and `--exclude`. `show <name>` prints the description, the source path, any declared `license` and `compatibility`, and the `agentsync-use-when`, `agentsync-not-for`, and `agentsync-requirements` metadata, labelled as unverified annotations. `check` reports missing or malformed required `SKILL.md` metadata without changing what `sync` accepts.
- **`agentsync skills catalog list|show --catalog FILE`** (experimental). It reads a hand-curated TSV of skill cards; `--source ALIAS=LOCAL_REPO` reads the name and description from the `SKILL.md` at the pinned commit in a local Git repository. It is read-only: nothing is fetched, installed, or run, and curator notes are shown as unverified. The format is in `docs/skill-cards.md`.
- **`agentsync mcp list|show|validate|render --library DIR`.** It inspects a local catalog of `<id>/manifest.json` files; `library.mcp.path` in `.ai/agent_sync.yaml` saves passing the directory each time. `render <id>[@variant]` prints the selected connection as an MCP source. Nothing starts a server or contacts an endpoint. The manifest format is in `docs/mcp-library.md`.
- **`agentsync mcp use <id>[@variant] --tool <slug>`.** It previews the per-tool MCP source it would write. `--apply` creates `.ai/src/tools/<slug>/mcp.json` with a backup `rollback` can restore, and refuses when an MCP source already exists; `--merge --apply` adds the server to an existing per-tool `mcp.json`, and `--replace <id>` overwrites a different entry with the same ID. For Kimi it writes Kimi's native form. `sync` stays a separate step.
- **A pilot MCP catalog in the repository**, `catalog/mcp/`, with Microsoft Learn, Context7, and Octocode. It is opt-in: pass its path to `--library`; the binary does not embed it.
- **MiniMax Code (`minimax`).** `agentsync enable minimax` writes `AGENTS.md` with inlined rule references and the project `.mcp.json`, the two surfaces MiniMax documents; skills, commands, subagents, settings, and hooks are not synced to it. `list` counts 14 tools. MiniMax shares `.mcp.json` with Claude Code, and `sync` stops before writing when their effective MCP sources differ. When another enabled tool also writes `AGENTS.md`, the rule references stay. `profile add` and `sync` refuse MiniMax profile variants because it reads only project-root files, and `adopt` refuses `AGENTS.md` while MiniMax is enabled, since the file carries generated references.

### Changed

- **Every command hint is coloured the same way.** The sync log's version-pin hints (`agentsync update <pinned>`, `agentsync upgrade-config`), its drift and first-sync advice, the legacy-layout warning, and the `run agentsync init` / `agentsync sync` errors of `profile`, `adopt`, and `doctor` print the command in cyan on a terminal, as `doctor` and `list` already did, and drop the quotes around it. Piped output is unchanged apart from those quotes.
- **`sync --help` names the command.** The usage opens with `Usage: agentsync sync [OPTIONS]` instead of the `sync.sh` header the Bash engine printed.
- **Every command's `--help` has one shape.** A bold `agentsync <command> — <what it does>` line, then USAGE, DESCRIPTION, OPTIONS, and EXAMPLES in green with the option column aligned, the way `agentsync help` already read. `check`, `list`, `disable`, `resolve`, and `doctor` gained a help of their own where they had answered with the top-level command list; `show` and `diff` print theirs instead of that list; `add mcp --help` moved from stderr to stdout. A refusal for a missing argument names what is missing (`Error: missing <name> for rule`) before the usage.
- **`doctor` ends with a blank line, not a rule.** The `────` line before the summary is gone; the CLI output rules forbid rules between sections.
- **`enable` exits 1 when a tool is unknown**, after enabling the ones it knows, so a misspelt slug fails a script instead of passing silently.
- **`refresh` names the templates by release.** The header reads `Templates: shipped with agentsync v0.40.0` where it printed the engine-internal `/<agentsync>/lib/templates`.
- **`sync` stops when tools sharing `AGENTS.md` have different sources.** Codex, Cursor, Windsurf, OpenCode, and MiniMax Code all write the root `AGENTS.md`. When a per-tool override gave one of them a different source, whichever tool synced last silently overwrote the others. Now `sync` fails before writing (`Agents destination AGENTS.md is shared by … but their sources differ`); `--only` checks only the tools it selects.
- **`add skill` checks the name.** A name that is not 1–64 lowercase letters, digits, or single hyphens, the Agent Skills format, is refused with exit status 1.
- **The shipped `prompt-engineering` skill is updated** with current model references and more snippets; `refresh` brings it into a project.

### Fixed

- **`upgrade-config --help` re-pinned the project.** The command took no arguments, so `--help` ran it. It prints its usage now, and an unknown argument is refused with exit status 2 before anything is written.
- **`generate --help` and `release --help` answer with usage.** `generate` treated the flag as the project description; `release` refused it as an unknown bump type.
- **`setup-hooks` repairs a hook an older release installed.** It found the marked block and stopped, so a hook that still ran `bash lib/sync.sh` or a `dart` wrapper kept failing after an upgrade. Running `agentsync setup-hooks` now rewrites an outdated block in place (`Updated AgentSync hook in post-checkout.`) and leaves the rest of the hook as it was.
- **`setup-hooks` failed in a removed working directory** even with `AGENTSYNC_REPO_ROOT` set, though it never uses the working directory then.

### Internal

- `serde` and `serde_json` are dependencies now, for the strict parsing of MCP manifests and sources (duplicate keys, nesting and size limits). YAML still goes through `yaml_subset`.
- The command modules share one set of write and directory-walk helpers in `cli/mod.rs`, and `main` resolves the project root in one place.

## 0.39.0

### Breaking

- **The sync log goes to stderr.** `sync` and `sync --workspace` print their `[INFO]`, `[WARNING]`, `[ERROR]`, and `[DONE]` lines on stderr, where cargo and git put theirs. Stdout stays empty unless you pass `--json`. A script that read the log from stdout gets nothing now: give it `--json`, or `2>&1` if it wants the human log. The JSON object is the contract, and a field is only ever added to it short of a major version. The human log may change again.

### Added

- **`sync --json`.** After a successful run, one line on stdout: `{"dry_run":false,"synced":2,"total":13,"skipped":["Cursor",…],"written":[".claude/rules/core.md",…],"preserved":0,"backup":".ai/backups/<id>"}`. `written` holds the root-relative paths this run wrote, `preserved` counts the user files it left in place, and `backup` is `null` on a dry run. A fresh `--if-stale` run reports `total` 0. A failed run prints no object; the exit status and stderr say what went wrong.
- **`sync --quiet` (`-q`).** Only warnings, errors, and the closing `[DONE]` line, for hooks and CI steps that want one line per run. `--workspace` passes `-q` and `--json` on to every project.

### Changed

- **A shorter sync log.** The `═══` rules, the start banner, and the `[SUCCESS] <tool> complete` line after every block are gone; the closing `[DONE] Synced 2/13 tools (11 skipped)` already says what ran. A real run no longer lists the skipped tools by name; `--dry-run` and `--json` still do. Counts read `(8 updated, 1 removed)`, singular for one, with no `0 cleanups`.
- **No engine-internal paths in the log.** A merged source is named by what it is, `rules/ → .claude/rules/`, and a shipped file by its template path, `templates/guard/claude.sh`, where the log used to print `/<agentsync-overlay>/…` and `/<agentsync>/lib/…`.
- **An error and its hint stay together on stderr.** Running `sync` from inside `.ai/` and passing an unknown option both printed the error on stderr and the help on stdout. `--help` alone still prints usage on stdout.
- **`list`, `check`, and `version` ignore extra arguments again.** Since 0.37.0 they refused them with exit status 2; that came from the argument parser the port used, not from AgentSync. `sync -- --dry-run` answers `Unknown option: --` as the shell version did, instead of silently running a dry run.

### Fixed

- **The backup path was absolute on Windows.** When the working directory is spelled with an 8.3 short name (`RUNNER~1`) or reached through a symlink, the backup snapshot lives under the canonical root, and both the rollback hint after a failed sync and the `backup` field of `sync --json` printed it in full. Both read `.ai/backups/<id>` now.

### Internal

- The argument parser crate is gone. Each command reads its own options as its Bash `cmd_*` did, and `cli::Command` is one exhaustive enum over the command words.
- The library is grouped by role: `config/`, `engine/`, `transaction/`, and `output/`, with `error`, `paths`, `text`, `project`, and `cli` at the root. `init`, `refresh`, `doctor`, and `render` are directory modules now. Public paths and tests are unchanged; `.claude/rules/architecture.md` has the map.

## 0.38.1

### Changed

- **No emoji in what the tool prints.** The level tags say the level in words — `[INFO]`, `[WARNING]`, `[ERROR]`, `[SUCCESS]`, `[DONE]` — and keep their colour on a terminal; report markers are the narrow `✓`, `✗`, `!` and `·`. Two reasons beyond taste: the warning marker carried the emoji presentation selector, which Unicode Annex #11 makes Wide, so it shifted every column after it on terminals that honour that; and a bare glyph gives a screen reader nothing to read. The folder in front of every copied file is gone too — it was the one glyph that reached pipes and CI logs, since the rest only appeared when colour was on. A script matching the old glyphs needs updating, which is why the output of a command is not a contract: use the exit status.

### Fixed

- **`agentsync update` printed raw Markdown links in the release notes.** A link now shows its text, plus its target when the target says something the text does not. The renderer stripped bold and code spans but had never seen a link, because no release notes had used one until 0.38.0's.

## 0.38.0

The Bash engine is gone. 0.37.0 shipped the Rust binary while the shell implementation stayed in the repository as the reference; this release deletes it, and the test suite that graded both engines against each other is now Rust too. Nothing about using AgentSync changes — the commands, the config, and the generated output are the same — but the tool is faster than it has ever been, and it no longer needs `bash` on your machine.

### Changed

- **Everything is faster, and `check` is a different tool.** On the repository's benchmark fixture (389 source files, 13 tools, 3465 generated files), on one machine: `check` 73.6 s → 0.25 s, `sync` 67.5 s → 2.5 s, `sync --if-stale` 0.18 s → 0.01 s, `list` 0.57 s → under 0.01 s. Startup alone fell from 38 ms to 5 ms. A `check` that took over a minute could not sit in a pre-commit hook or a CI gate; at a quarter of a second it stops being a decision. The old engine spawned a process for every value it read from YAML, every path it resolved, and every hash it computed — that is the cost that disappeared. Method and the full table are in `docs/perf/2026-09-19-rust-result.md`, measured against the baseline in `docs/perf/2026-09-13-bash-baseline.md`, recorded before the migration began.
- **No runtime dependencies.** The binary needs neither `bash` nor coreutils. The repository keeps 390 lines of shell where a binary cannot serve: `install.sh`, which runs before a binary exists, and the guard hook, which runs in a teammate's checkout where the CLI is not installed. It held 18,566 lines before.
- **Windows is a first-class platform, not a compatibility layer.** The binary runs natively; Git Bash is no longer involved. Its CI went from twelve sharded jobs to one.

### Fixed

- **The Windows binary works.** 0.37.0 shipped one, but it failed on the first command it was given: the engine prepended `/` to a drive-rooted working directory, so every run ended in `Error: Directory not found: .`. Four more faults sat behind that one — `PathBuf` joins put backslashes into paths the engine builds as strings, a backup could not set a file's modification time because the handle was not open for writing (`Could not back up sync targets`), `shell-init` did not recognise `$SHELL` when it arrived as `C:\…\bash.exe`, `add mcp` had a backslash argument rewritten as a path, and `AGENTSYNC_EXTERNAL_SOURCE_ROOTS` split at a drive letter's colon. Windows now runs the same suite as Linux and macOS, unsharded, in CI.
- **The interactive prompts lost every second answer on Linux.** `init`'s wizard read a line through a buffer that swallowed whatever you typed next, so the answer to question two vanished and the wizard used the default. Answers are read a byte at a time now, as the shell did.
- **A source install that was already up to date never moved to the binary.** `agentsync update` returned "Already up to date!" before reaching the step that replaces a git checkout with the downloaded binary, so an install from before 0.37.0 stayed on the old layout however often you ran it.

- **`doctor` could report a file as clean while a live secret sat in it.** Two faults, both inherited from the Bash scanner and both now fixed: a line was discarded whole if it contained a `${VARIABLE}` anywhere, so a real token beside an unrelated placeholder was never reported; and only the first matching pattern's hits were shown, so an AWS key hid a Slack token on the next line. The scanner now reads each line with the `${...}` spans removed and reports every line that matches any pattern. Expect `doctor` to find things in projects it previously passed.
- **`agentsync update` showed the wrong changelog for a version whose number is a prefix of another.** `## 9.9.90` opened the section for `9.9.9`, which then ran to the end of the file. The heading now has to match the whole version.
- **`agentsync resolve` is read-only without a terminal, as it says it is.** It cleared `.ai/.pending-resolutions.yaml` before reaching the check that prints "read-only — not a TTY", so a CI run threw away the queue it was told it would only report on.
- **`dedupe` and `doctor` could hang forever on Windows outside a git repository.** Both look for a parent `.ai/src/` by walking up from the project, and the walk stopped at `/` — a root Windows does not have. Reaching `C:\`, whose parent is itself, the walk never ended: the command sat there consuming a core until it was killed. It needed a project with no `.git` anywhere above it, which is why it survived every release: the test suite created its fixtures with `git init`, so the walk always hit a repository boundary first and turned back. The port of that suite to Rust built bare directories instead, and the first Windows run found it in a minute. The walk now stops at whatever the platform's root is.

### Internal

- The bats suite is retired. Its 727 cases across 41 files became Rust integration tests on `assert_cmd`, one commit per file, each test named after the case it replaces; `cargo test` is now the whole suite at 1066 tests and runs on Linux, macOS, and Windows in one job per platform. bats and GNU parallel are out of CI.
- The migration is complete: `docs/specs/2026-09-12-rust-migration-design.md` and the plans under `docs/plans/` record every phase, its receipt, and the accepted deviations from the Bash engine's behaviour.

## 0.37.0

AgentSync is now a single static binary. The engine was rewritten in Rust command by command behind the Bash dispatcher, each command proven byte-identical to its Bash predecessor by the bats suite and a parity harness, and this release is the first to ship it: macOS (Apple silicon and Intel), Linux (x86_64 and arm64, statically linked), and Windows (x86_64, no Git Bash needed).

### Added

- **Binary installs.** `curl -fsSL https://raw.githubusercontent.com/yelmuratoff/agent_sync/main/install.sh | bash` downloads the release archive for your platform from GitHub Releases, verifies its sha256, places `~/.agentsync/bin/agentsync`, and links it. `AGENTSYNC_VERSION=<tag>` still pins; a tag older than this release has no archive and installs from source as before. The cargo-dist installers (`agentsync-installer.sh`, `agentsync-installer.ps1`) ship alongside.
- **`agentsync update` replaces the binary.** The latest release, or `update <version>`, is downloaded and verified, the new binary's embedded tool catalog is compared with the running one against your overrides (`--strict` fails on a conflict, the queue goes to `.ai/.pending-resolutions.yaml` for `agentsync resolve`), the release's changelog is printed, and the binary is swapped in place. An existing source install moves to the binary by itself the next time `agentsync update` reaches a release that ships one.
- **Releases are built by cargo-dist**: five archives, sha256 sums, artifact attestations, and the installers, dispatched by the auto-tag workflow for the tag it creates from `VERSION`.

### Changed

- The update banner and the project-format notice print from the binary. The banner reads `.update_cache` beside the install's `bin/`, refreshed in the background from the latest GitHub release.
- `sync` and `check` no longer fork: the 13-tool sync that took 6 seconds in Bash runs in a fraction of a second, so `check` in a pre-commit hook or a CI gate stops being the slow step.
- Directory listings, tool slugs, and globs read in byte order where Bash followed the locale's collation; every other accepted difference from the Bash engine is listed in `docs/specs/2026-09-12-rust-migration-design.md` under "Accepted deviations".

### Internal

- `bin/agentsync.sh` and `lib/` stay in the repository as the Bash reference and parity harness until the next phase deletes them; `AGENTSYNC_NATIVE=0` still forces them for a ported command.

## 0.36.0

Sources can live outside the project, rollback no longer discards what changed after the operation it undoes, and `agent_sync.yaml` gains a strict engine pin and a retention mode that never prunes recovery data. Every change in this release started as a pull request from [@lunetics](https://github.com/lunetics) (#9, #10, #11, #12, #13).

### Added

- **`source.*` in `agent_sync.yaml` may point outside the project.** A separately maintained tree of rules, skills, commands, subagents, and tool configs can feed several projects: set `source.rules: /home/me/agentic/.ai/rules`, or a `../` path, and trust the tree with `export AGENTSYNC_EXTERNAL_SOURCE_ROOTS=/home/me/agentic` (colon-separated). `source.tools` supplies both the tool YAML and its `settings`/`hooks`/`mcp` payloads, and `check` and `sync --if-stale` read the same roots. Trust comes only from that variable, never from the project, so syncing a cloned repository — the `shell-init` hook included — cannot read files from wherever its config points; an untrusted outside root stops `sync` and `check` before they write. Only values written explicitly in the project config widen where sources are read from. Defaults, auto-detected layouts, and symlinks under `.ai/src/` keep the project boundary, and `/`, your home directory, and the project root or any directory containing it are refused. Commands that write tool config (`customize`, `profile`, `adopt`, `simplify --apply`, `enable --scaffold`) refuse a `source.tools` outside the project instead of editing it. (#9)
- **`rollback` refuses to overwrite what changed after the operation.** Every init, sync, and rollback now records the state its targets were left in (`after.tsv` in the snapshot). Rollback compares the targets against that record first. A file added, edited, or removed since then, such as a `.claude/settings.json` created after a skills-only sync, stops the rollback before anything is written, naming the first changed path. `--force` restores anyway, still taking the safety snapshot. Snapshots made by earlier versions have no record; they restore as before, with a warning that later changes cannot be detected. (#13)
- **`version_pin.mode: strict` makes a local-outputs pin mismatch fatal.** Committed outputs already stop `sync` and `check` when the running engine differs from `agentsync_version`; local outputs only warned. Nest `mode: strict` under `version_pin:` (or write the shorthand `version_pin: strict`) to stop there as well. `warn` is the default, and any other value stops `sync` and `check` before they write. (#10)
- **`backup.retention: preserve` keeps every existing snapshot and staging entry.** Setting both `AGENTSYNC_BACKUP_LIMIT` and `AGENTSYNC_BACKUP_MAX_AGE_DAYS` to `0` still let the next backup sweep abandoned `.tmp.*` staging, which can hold the only copy of an interrupted run's state. Under `preserve`, init, sync, and rollback prune nothing that existed when they started; new snapshots are still created. `bounded`, the previous behaviour, stays the default. An invalid policy or bound now stops init, sync, and a rollback restore before they write, where it used to surface as a "Could not prune" warning afterwards; `check` and `rollback --list` skip the validation. (#12)

### Fixed

- **A symlink under `.ai/` can no longer copy a file from outside the project into outputs.** Sync followed links while copying, so a cloned repository could commit `.ai/src/rules/notes.md -> ~/secrets.md` and the next `sync` — or the `shell-init` hook on `cd` — wrote that file into `.claude/rules/`. Before reading anything, `sync` and `check` now resolve every link under `.ai/` and the configured sources, following chains and directory links, and stop before writing when a target lies outside the project and outside `AGENTSYNC_EXTERNAL_SOURCE_ROOTS`, naming the link. **If you link a shared rules or skills tree into `.ai/src/` from outside the project, list that tree in `AGENTSYNC_EXTERNAL_SOURCE_ROOTS`.**
- **A mistyped `AGENTSYNC_CONFIG_PATH` no longer falls back to another config.** A path to a missing file printed a warning and went on with `.ai/agent_sync.yaml`, so a typo synced with a different policy than intended. `sync`, `check`, `init`, a rollback restore, and the commands that read the config (`list`, `show`, `doctor`, and the rest) now stop with `AGENTSYNC_CONFIG_PATH is set but file not found`. (#11)
- **Deleting `agent_sync.yaml` no longer wipes every tool's outputs.** Without a config no tool was enabled, so the cleanup defaults removed every generated file and the manifest, and sync still exited 0. A write sync with no config and no enabled tool is now refused before anything changes, naming `agentsync enable <tool>`. A project that enables tools in their own `.ai/src/tools/<tool>.yaml` still syncs without a config. (#11)
- **`check` honours a config outside `.ai/`.** Its isolated sync copies only `.ai/` and the managed outputs, so a relative `AGENTSYNC_CONFIG_PATH` such as `config/agentsync.yaml` was looked up again inside the copy. It fell back to `.ai/agent_sync.yaml` there, and `check` reported outputs as missing that `sync` had generated correctly. `check` now hands the resolved path to that sync.
- **`agentsync update` renders the changelog for a terminal.** Entries reached the user as literal `**bold**` and backticks, and each paragraph ran off the window as one line. Inline markers are stripped and prose wraps to the terminal width.

### Internal

- One helper, `project_config_path_r`, locates the project config for `sync`, `check`, the backup policy, and the read-only commands; each reports a missing explicit path in its own voice.
- The tool-override directory is resolved once per process, keeping the configuration lookup on sync's hot path free of subshells.

## 0.35.2

No engine changes. 0.35.1 ships correct code, but its CI was red on the Windows and macOS runners; this release puts the tag on a commit that is green on all three platforms.

### Internal

The suite now passes on the Windows and macOS runners, not only on a developer's machine. Five unrelated causes, every one in the tests rather than the engine:

- **`install.bats` pinned the installer to this repository's release tags**, which `actions/checkout` never fetches. It builds its own tagged fixture origin instead.
- **`paths.bats` asserted `dirname` parity for a pathname of exactly two slashes** — implementation-defined in POSIX, and answered differently by BSD and MSYS.
- **`paths.bats` also built an expectation by concatenating the project root**, which macOS returns containing `//` because `$TMPDIR` ends in a slash.
- **`format_migration.bats` hashed with `shasum`**, which Git Bash does not ship.
- **Twenty-six other comparisons hashed the same way**, so on Windows both sides came back empty and the assertion proved nothing. All hashing goes through one helper that prefers `sha256sum`.

Also:

- **`tests/paths.bats` covers the path resolvers directly.** The module holds the containment check that keeps sync from writing outside the project root, and had no tests of its own.
- **`team_workflow`'s git steps say what failed.** The fixtures discarded git's stderr, so an intermittent macOS failure surfaced only as `status 128`. Each step now reports its name, status, and output. That failure is still undiagnosed — it reproduces on no local run.

## 0.35.1

### Performance

- **`sync` does about a quarter less work, and the test suite close to half.** Sync was fork-bound — system time ran at twice user time — spending more on `$(...)` command substitutions than on the work inside them. The path-resolution chain, the repo-relative and display-path helpers, the manifest recorders, and the backup target validators now return through `$REPLY` rather than a subshell; `dirname` and `basename` became parameter expansion; `cd -P && pwd` is memoised per directory; and an already-canonical path skips normalisation entirely. On a 13-tool project that is −22.8% user+sys CPU, ahead in every round of an interleaved A/B, and the bats suite drops −45.3% — a small sync pays proportionally more of the fixed cost that went away. Generated output is byte-identical across all 234 files, and every echo-returning helper was kept, so no call site outside these paths changed.

### Internal

- **`install.bats` builds its own tagged origin repository.** The tests pinned the installer to real release tags of this repository — which a developer's full clone has and `actions/checkout` does not — so they passed locally and failed on CI. They now stand up a fixture repo with its own `main` branch and tags, and a guard test asserts the fixture really publishes them so the rest cannot pass vacuously.
- **`paths.sh` has direct test coverage.** It holds the containment invariant that keeps sync from writing outside the project root, yet was only exercised through other commands. `tests/paths.bats` checks the lexical primitives against `dirname` and `basename`, the normalisation fast path, symlinked ancestors, and every escape rejection.
- `backup.sh` now depends on `paths.sh`, so `rollback`'s module list and the backup tests load it.

## 0.35.0

Team setup in one command: generated outputs travel through git, so everyone except the person editing the rules runs nothing.

### Added

- **`outputs: committed | local` in `agent_sync.yaml`, committed by default for new projects.** Committed keeps the generated tool files and `.ai/.sync-manifest` in git, so a teammate gets current rules from `git pull` alone and CI gates drift with `agentsync check`. Local is the previous behaviour: both are gitignored and every clone regenerates. Profile config homes stay gitignored either way, being personal. A project without the key keeps local semantics, or committed when it already set `gitignore.update: false`.
- **`agentsync init` adopts the tool config a project already has.** It copies an existing `CLAUDE.md`, `.claude/rules/*`, and the rest into `.ai/src/`, so the first sync reproduces them instead of replacing them with the shipped templates. Two destinations that map to one source keep the first and report the rest rather than clobbering. With a `.github/` directory it also offers a GitHub Actions gate that runs `agentsync check` against the pinned version, shipping a disabled `autofix` job for teams who would rather have CI run `sync` and commit the outputs into the pull request. `--existing replace`, `--ci github`, and `--no-sync` drive the choices non-interactively.
- **The engine version pin is enforced, not just reported.** Committed outputs are only reproducible when every machine runs the same engine, so `sync` and `check` stop when the running version differs from `agentsync_version`, naming the two ways out; in local mode it stays a warning. The installer honours `AGENTSYNC_VERSION=<version>`, and `agentsync update <version>` pins an install to a release tag.
- **Generated files are guarded against agent edits.** Tools can declare a `guard` target; Claude's is a generated `PreToolUse` hook at `.claude/hooks/agentsync-guard.sh`, registered by the base settings. It matches the path against `.ai/.sync-manifest` and exits 2 — blocking the write — naming the source to edit and the `adopt` command for an edit already made. Plain POSIX `sh`, so it works for a teammate who never installed the CLI; replace it at `.ai/src/tools/claude/guard.sh`. The shipped `AGENTS.md` and `rules/core.md` also state that instructions live in `.ai/src/`.
- **The `agentsync` skill is engine-owned, so upgrades reach every project.** It documents AgentSync itself but was copied into `.ai/src/skills/` at `init` and only updated if someone ran `refresh`, leaving a team's agents on the behaviour of an older release. It now ships with the engine and is resolved at sync time, below any `shared:` parent and below a project copy, which still wins. `base_skills: false` drops the layer.
- **`format:` in `agent_sync.yaml` records which migrations a project has been through.** A counter separate from `agentsync_version`, bumped only when a project needs a step, so the reminder appears when something applies and never otherwise. `init` writes it and `migrate --apply` records it; nothing else touches it. A project behind the engine is flagged on the next interactive command and by `doctor`.
- **`agentsync migrate` retires a shadowing copy of an engine-owned skill.** `--apply` removes the copy when every file still matches the hash recorded in `.ai/.template-manifest`, and keeps an edited copy as the deliberate override it is. Either way the format revision is recorded.
- **`agentsync adopt <file>` works before the first sync** — it previously refused without a manifest, exactly the state in which a project's own `CLAUDE.md` needs adopting. `--all` still needs the manifest to find drift.
- **`profile_scoped: false` on a target** keeps the base destination instead of rewriting it into a profile's config home, for project-level content every home shares.
- **An explicit-only command stays explicit-only in Codex.** Codex ignores Claude's `disable-model-invocation` frontmatter and keeps the generated `command-*` skill in its implicit selection list. Sync now emits `agents/openai.yaml` with `allow_implicit_invocation: false` beside the generated `SKILL.md`, and removes it again once the flag is gone.

### Fixed

- **A teammate's `git pull` no longer reads as a manual edit.** The README told users to commit `.ai/.sync-manifest` while sync gitignored the outputs it describes, so after someone pushed a rule change everyone else's next `sync` aborted with "Manual edits detected" — and the `post-merge` hook hit the same abort and silently skipped, so the rule never arrived. The manifest now always shares the git status of the outputs it describes.
- **`setup-hooks` installs where git actually looks.** It wrote straight into `.git/hooks`, so on a repo with `core.hooksPath` set — husky, lefthook, a global hooks directory — it created a file nothing would ever run. It now resolves the directory with `git rev-parse --git-path hooks` and prints the snippet to add to the managed hook instead.
- **`doctor` reports a guard the tool never invokes.** A settings override written before the guard shipped drops its registration, leaving the script generated and never called — protection that looks present and is not.
- **A config-home profile no longer gets a dead guard copy.** Each profile received its own `hooks/agentsync-guard.sh` while its settings pointed at the project-level path; one script now serves every profile, and a profile-only sync writes it.
- **`init --tools <tool>` no longer scaffolds into the deprecated layout.** Payloads landed in `.ai/src/settings/`, legacy since 0.11, so the first `sync` on a fresh project told the user to migrate it. They now land in `.ai/src/tools/<tool>/<resource>.<ext>`.
- **A first sync says what it replaces.** With no manifest it overwrote a project's pre-existing tool config silently; it now lists those paths and points at `rollback` and `adopt`.

### Changed

- **`agentsync init` runs the first sync,** so a fresh project has its outputs before you commit. Pass `--no-sync` to skip it.
- **`setup-hooks` installs the hooks that suit the outputs mode.** Committed outputs travel through git, so `post-merge` and `post-checkout` would only fight the incoming files; `pre-commit` instead re-syncs and fails the commit when that changed a generated file, listing the paths to stage. Local outputs keep `post-merge` and `post-checkout`, with `--pre-commit` still optional. Every installed hook honours `AGENTSYNC_SKIP_HOOKS=1`.
- **`rules/git.md` no longer tells the agent to gitignore generated agent config** — under committed outputs that is exactly wrong. It defers to `outputs:` and asks for the generated files in the same commit as the source change.

### Internal

- Tests no longer inherit the developer's environment: the shared helper unsets the `post_sync` trust variables and pins `GIT_CONFIG_GLOBAL` / `GIT_CONFIG_SYSTEM`, so a global `core.hooksPath` cannot decide an outcome. CI also lints `install.sh`.

## 0.34.0

### Fixed

- **Rewriting a managed file no longer tightens its permissions.** Files written through a temporary sibling and renamed into place — `.gitignore`, `.ai/agent_sync.yaml`, `.mcp.json`, and the manifests — inherited the temporary file's `0600` instead of keeping their own mode, because a rename replaces the inode along with its permissions. A project's `.gitignore` silently dropped from `0644` to `0600` on the first sync that rewrote the generated block. The original mode is now carried across, and a destination its owner cannot write is left to the temporary file's mode rather than producing staging nothing can write.
- **Interrupting a command no longer leaves garbage behind — or a half-written project.** Every handler was armed on `EXIT` only, and a Bash script killed by a signal never runs its `EXIT` trap. Ctrl-C during `sync` therefore leaked the shared-overlay tree (a full copy of `.ai/src`), leaked the backup staging directory (a full copy of the managed write set, inside the repository), *and* skipped the transactional restore, leaving destinations half-written. Handlers now cover `INT`, `TERM`, and `HUP`, pass the signal's status explicitly instead of reading `$?` — which inside a signal handler holds the last completed command's status and is frequently `0` — and re-raise so the caller still sees a real interrupt.
- **Orphaned backup staging is reclaimed.** A run killed mid-snapshot left `.ai/backups/.tmp.<op>.*` in the project permanently: the names are dot-prefixed, so `backup_prune`'s glob never saw them, and it also required a `.complete` marker they never get. `backup_create` now sweeps staging and metadata temporaries older than 24 hours, a threshold that leaves a concurrently running sync's staging untouched.
- **Temp files no longer accumulate in `$TMPDIR`.** `agentsync_legacy_warn_<pid>` was written on every invocation that saw a legacy payload override and removed by nothing; a dozen other `mktemp` sites had no cleanup on their error paths. All scratch now lives in one per-run directory reclaimed on every exit path, and atomic-write staging files are registered for cleanup where they must stay beside their destination.
- **Shared and profile overlays are cleaned up under any `TMPDIR`.** Teardown only removed paths matching `/tmp`, `/private/tmp`, or `/var/folders`, so a custom `TMPDIR` (`TMPDIR=$RUNNER_TEMP` on CI) leaked a full `.ai/src` copy per sync and printed a "looks suspicious" warning. The guard now checks provenance — the overlay must live in the directory this run created — which is both correct under any `TMPDIR` and narrower: the old check would have removed any unrelated directory that happened to sit under `/tmp`.
- **`--workspace` no longer treats vendored and internal directories as projects.** `find_workspace_ai_dirs` walked everything below the current directory, so a dependency shipping its own `.ai/` — a `node_modules` package, anything under `.git/` — was synced as if it were the user's project, writing config a package manager discards. `.ai/` is now pruned once matched, which also stops a backup snapshot mirroring `.ai/src` from being reported as a second project nested inside the first.
- **A run of failing syncs no longer accumulates snapshots.** Pruning ran only on the success path. It now also runs after a failed `sync` or `init` whose restore completed — but never when the restore failed, since the store then holds the only copy of the pre-operation state.

### Changed

- **Backups are bounded by age as well as count.** A snapshot is retained only if it is among the newest `AGENTSYNC_BACKUP_LIMIT` (default 10) *and* younger than the new `AGENTSYNC_BACKUP_MAX_AGE_DAYS` (default 30). Setting either to `0` disables that bound alone. The newest snapshot is always retained, so rollback stays available however long a project sits idle, and a snapshot whose name carries no parseable timestamp is never aged out.
- **`AGENTSYNC_BACKUP_LIMIT=0` no longer means "keep everything".** It now disables only the count bound; snapshots older than 30 days are still removed. To keep the full history as before, set `AGENTSYNC_BACKUP_MAX_AGE_DAYS=0` as well.

## 0.33.5

### Fixed

- **`agentsync check` now works in a global install.** Check copied the whole project root into its temporary workspace. For a global install that root is `$HOME`, so it tried to read OS-protected directories (`~/Library`, `~/Pictures`) and multi-GB tool caches — aborting with `Failed to prepare temporary workspace` on macOS, and filling the disk on the way there. It now copies only the `.ai/` source tree plus the outputs recorded in `.sync-manifest`, excludes `.ai/backups/`, and compares just those managed paths instead of diffing the entire root. Files beside the project no longer count as drift, and a partial copy is reported as a failure instead of passing as "in sync".

## 0.33.4

### Fixed

- **Sparse shared overlays no longer abort sync.** Nested workspace sync now skips optional shared categories that have no files instead of exiting under `set -e`; rules, skills, commands, and agents can be listed in `shared.inherit` before their source directories exist.

## 0.33.3

### Changed

- **English-only skill discovery examples:** replaced Russian trigger phrases in bundled skill descriptions and writing guidance with English equivalents, removing redundant examples. Existing projects receive the updated templates through `agentsync refresh`; new projects receive them through `agentsync init`.

## 0.33.2

### Added

- **`agentsync init --no-templates`.** Scaffold the usual `.ai/src/` section directories (and an empty `AGENTS.md` when the agents section is selected) without copying shipped starter rules, skills, commands, or subagents. Pairs with `--no-detect` for nested `.ai/` trees or migrations where you bring your own content; use `agentsync refresh` later to adopt shipped templates selectively.

## 0.33.1

### Added

- **AI-assisted project migration prompt.** `agentsync migrate` now prints a self-contained prompt and automatically copies it with the available macOS, Linux, Wayland, X11, or Git Bash clipboard tool. The prompt grounds an AI migration in the project's pinned version, every relevant official changelog entry, and the latest matching documentation and templates; it requires a recoverable checkpoint, minimal source-of-truth edits, supported AgentSync migration commands, and `doctor` / `sync` / `check` verification. The historical layout migration remains available as `migrate --legacy` for dry-run and through the backwards-compatible `migrate --apply [--yes]` route.

### CI

- **Windows backup safety tests now exercise real symlinks.** Git Bash normally turns `ln -s` targets into copies, so the backup containment tests were checking ordinary in-project directories and incorrectly reporting three security guards as failures. The shared fixture now requests native symlinks with `MSYS=winsymlinks:nativestrict` and verifies each link before the test continues.

## 0.33.0

### Added

- **Transactional `init` and `sync`.** Before either command changes managed files, AgentSync now creates a complete snapshot under the Git-ignored `.ai/backups/` store. If the operation fails midway, it automatically restores the exact pre-operation state, including removing destinations that did not exist before the run.
- **Manual rollback for accidental syncs.** `agentsync rollback` restores the latest snapshot, while `agentsync rollback <backup-id>` selects an older one. Use `rollback --list` to inspect available snapshots, `--dry-run` to preview every restore/remove action, and `--yes` for non-interactive recovery. Every rollback first creates its own safety snapshot, so the rollback can itself be undone.

### Changed

- **Bounded, low-noise backup lifecycle.** Successful operations retain the latest 10 complete snapshots by default; set `AGENTSYNC_BACKUP_LIMIT` to another non-negative value or `0` for unlimited history. Dry runs, fresh `sync --if-stale` calls, drift-preflight failures, and isolated `agentsync check` runs do not create snapshots.
- **Recovery covers the full managed write set.** Tool and profile destinations, disabled-tool cleanup targets, `.ai/.sync-manifest`, and AgentSync's `.gitignore` block are restored together. Arbitrary side effects from trusted `post_sync` hooks outside declared destinations remain outside the rollback boundary.

### Security

- **Snapshot restore stays inside the project.** Backup creation and rollback reject the repository root, the backup store itself, paths outside the repository, path traversal, and symlink escapes. Incomplete snapshots are ignored, metadata pointers are replaced atomically, and nested destinations are collapsed before copying.

## 0.32.0

### Added

- **First-class Kimi Code target.** AgentSync now syncs project instructions with inline rule references, native skills, commands as `command-*` skills, and canonical MCP configuration into Kimi Code's project layout. Kimi's built-in agents and global-only hooks remain outside project sync.
- **Complete OpenCode agent-layer target.** OpenCode now receives project instructions, inline rule references, skills, native commands, portable subagents converted to safe OpenCode Markdown, settings, canonical MCP, and an AgentSync-owned project plugin for hooks.
- **Ownership diagnostics for composed OpenCode configuration.** `sync`, `doctor`, and `adopt` now surface conflicting settings/MCP sources and multi-source outputs explicitly instead of silently choosing an owner.

### Changed

- **Canonical MCP now composes into OpenCode atomically.** The shared `mcpServers` map is validated, converted to OpenCode's local/remote schema, and merged into the top-level `mcp` field while preserving unrelated settings. A per-tool canonical MCP override still takes precedence over the shared source.
- **Tool support documentation now separates coding tools from model providers.** Kimi Code is documented as a standalone target, while Kimi models and GLM/Z.AI provider setups continue to use the target for their host tool. The documented ownership boundary excludes credentials, UI preferences, arbitrary OpenCode extensions, and Kimi's global runtime home.

### Security

- **Portable OpenCode subagents keep unknown tools denied by default.** Permission conversion grants only recognized portable tools; misspelled or unsupported tool names can no longer broaden a generated subagent's access.

## 0.31.0

### Added

- **Per-target category opt-out.** Set `targets.<category>.enabled: false` in a tool YAML to skip that category while continuing to sync the tool's other outputs. This is especially useful for `base:` profile variants that should inherit most destinations but let another config own one category.

### Fixed

- **Shell auto-sync now runs only at an AgentSync project root.** The `chpwd` hook previously walked up from every descendant to the nearest `.ai/src`, repeatedly checking the same parent workspace on ordinary navigation and resurfacing its drift errors. It now syncs only when the current directory itself contains `.ai/src`.

## 0.30.0

### Security

- **Post-sync hooks now require an out-of-repo trust signal.** A `post_sync` hook runs arbitrary shell from a tool YAML, and it used to be enabled by `post_sync.allow: true` in the project's own `.ai/agent_sync.yaml` — so cloning an untrusted repo and running `agentsync sync` could execute its hook. Enabling a hook now requires a signal outside the synced repo: `AGENTSYNC_ALLOW_POST_SYNC=true`, or `post_sync.allow: true` in the install-dir `config.yaml`. The in-repo `post_sync.allow` is no longer honored. `post_sync.skip: true` in the project file still disables hooks (skip only ever removes capability). **Action:** if you relied on in-repo `post_sync.allow`, export `AGENTSYNC_ALLOW_POST_SYNC=true` or set it in the global config.

### Fixed

- **Glob filters no longer collapse against the working directory.** `include`/`exclude` globs were word-split unquoted, so a pattern like `*.md` was expanded against the run's cwd (the project root) before matching. When the tested filename was absent from the cwd the real pattern was dropped — rules were silently skipped and previously-synced outputs swept. The split now runs with pathname expansion disabled (same fix in the inline-list YAML and frontmatter-tools parsers).
- **Empty/filtered rule sets no longer abort a merge.** `merge_rules_to_file` iterated an unguarded array; on bash 3.2 under `set -u` an empty array is a fatal "unbound variable", and it fired after the destination file was removed, so an empty rules dir could delete the old output and abort before the manifest was written (causing false drift on the next sync). The dead, same-bug `copy_rules` helper was removed.
- **No more spurious "Kept"/"Removed" churn on shared and nested destinations.** Two tools sharing a skills dir (Codex + Antigravity both write `.agents/skills`) made the non-generating tool warn `Kept …` for the other's generated `command-*` skills, and an AGENTS file written into a rules dir (`.amazonq/rules/00-context.md`) was swept then re-copied every run. The reserved `command-*` namespace is now always excluded from the skills sweep, and no sweep prunes a file the same run already wrote.
- **Generated command/agent TOML and Amazon Q JSON now escape quotes and backslashes**, so a `name`/`description` containing `"` no longer produces an unparseable file.
- **Deleting a source `.md` now removes its generated TOML/JSON.** The command/agent converters had no differential cleanup, so an orphaned `.toml`/`.json` lingered forever; they now sweep obsolete generated files (preserving user-added files, like the rules sync).
- **A misconfigured per-tool source no longer aborts the whole run.** A missing source (e.g. a typo'd `targets.rules.source`) made the copy/sync helpers return non-zero, which under `set -e` killed the entire run mid-pass — before the manifest was written — so later tools went unsynced and the next sync reported false drift. A missing source now logs its existing warning and is skipped; the run continues and finalizes normally.

### Changed

- **Faster sync.** Removed hot-path `$(...)` forks in the tool-config resolver (`get_tool_value_r` sets `REPLY` instead of echoing) and memoized the profile-tool lookup once per run. On an 11-tool sync this cuts user+sys CPU ~9% on macOS (larger on Git Bash/Windows, where forks cost more); output is byte-identical.

## 0.29.0

### Added

- **`agentsync adopt --all`:** batch-adopt every drifted (manually-edited) generated file back into `.ai/src/` in one pass, instead of naming each file. It scans the sync manifest for drift, previews the plan, and after a single confirmation promotes each edit and refreshes the manifest so the next `sync` is drift-free. Transformed targets that single-file `adopt` already refuses (header-injected rules, merged/inlined files, TOML/JSON-converted commands/subagents) are skipped and listed. When two edited outputs resolve to the same source with different content — e.g. `CLAUDE.md` and `GEMINI.md` both mapping back to `.ai/src/AGENTS.md` — both are skipped rather than one silently clobbering the other, so you adopt the intended file explicitly. Honours `--dry-run` and `--yes`.

## 0.28.5

### Changed

- **Disabled tools no longer clutter `sync` output:** a run where most tools are off used to print a `Skipping X (disabled)` line (and a blank line) for each one, burying the tool that actually synced under a wall of near-identical blocks. Skipped tools — disabled or excluded via `--only`/`--skip` — are now collected silently and reported once as a single `Skipped: a, b, c` line beside the final summary. A disabled tool whose stale output is actually removed still logs that cleanup.

## 0.28.4

### Changed

- **Readable `sync` logs:** every source→destination line now prints paths relative to the project root instead of absolute (`.ai/src/rules/ → .claude/rules/` rather than `/Users/you/.ai/src/rules/ → /Users/you/.claude/rules/`), with a `~`-relative fallback for paths outside the project. This drops the repeated home-directory prefix that pushed long lines onto a wrapped second row, so a global sync from `$HOME` is far easier to scan. Display only — what gets synced is unchanged.

## 0.28.3

### Fixed

- **`agentsync update` no longer misreports a moved tag as a network error:** when an upstream release tag had moved, the `--tags` fetch refused to overwrite the stale local copy and aborted the whole update with a misleading "Check your network connection". The install mirrors upstream and never owns tags, so the fetch now force-syncs them; a genuine fetch failure prints git's real error instead of always blaming the network.

### Changed

- **Releases are published as tags only:** the GitHub Release workflow has been removed, so cutting a version no longer creates a noisy GitHub Release entry. The annotated git tag still carries the CHANGELOG section as its message, and `agentsync update` continues to discover and pull new versions from tags — the update flow is unchanged.

## 0.28.2

### Changed

- **`shell-init` now recommends the `eval` form:** the help, README, and bundled skill now point to `eval "$(agentsync shell-init zsh)"` in your rc file rather than appending a frozen copy with `>>`. Eval-ing regenerates the hook from `agentsync` each session, so upgrades and fixes apply automatically without re-editing your rc (the same pattern as direnv/starship/zoxide). Appending the snippet directly still works for anyone who prefers to avoid the per-session call.

## 0.28.1

### Fixed

- **`shell-init` no longer breaks zsh on `cd`:** the auto-sync hook installed by `agentsync shell-init` ran an internal `cd`, which — being a zsh `chpwd` hook — re-triggered itself and aborted every directory change with `maximum nested function level reached`. The hook now points the sync at the project via `AGENTSYNC_REPO_ROOT` instead of `cd`-ing, and guards against re-entry. If you installed the hook from 0.28.0, remove the old block from your rc file and re-run `agentsync shell-init zsh >> ~/.zshrc`.

## 0.28.0

### Added

- **Auto-sync on directory change — `agentsync shell-init`:** prints a shell hook (`shell-init zsh|bash`, shell auto-detected from `$SHELL`) that runs `agentsync sync --if-stale` for the nearest `.ai/` project whenever you `cd` into it — and when you open a shell there — so generated rules/skills stop going stale between edits across many projects. It is a silent no-op when nothing changed, so it costs nothing in already-synced directories. Append it once: `agentsync shell-init zsh >> ~/.zshrc`. Set `AGENTSYNC_NO_AUTO_SYNC=1` to disable without removing the snippet.
- **`agentsync sync --if-stale`:** a cheap staleness probe that runs a full sync only when a source input is newer than `.ai/.sync-manifest`, and otherwise exits silently. It is the primitive the shell and pre-commit hooks build on, safe to call on every prompt.
- **`agentsync setup-hooks --pre-commit`:** optionally installs a pre-commit hook that runs `sync --if-stale` before each commit.

### Fixed

- **Git hooks now actually sync in real projects:** `setup-hooks` previously wrote a hook body that only invoked the in-repo `lib/sync.sh` or a `dart` wrapper, so in a normally installed project the `post-merge` / `post-checkout` hooks silently did nothing. They now call the installed `agentsync` binary (falling back to the in-repo engine), and every hook is non-fatal — a failed sync warns but never blocks the git operation.

## 0.27.2

### Changed

- **Faster `sync`:** the sync engine does the same work with about half the process forks. Manifest hashing now runs as a single batched pass instead of one `sha256sum` per file (drift-check and write), per-tool config lookups resolve without per-call subshells, hot-loop `basename`/`dirname` calls use Bash parameter expansion, and the enabled-tools set is computed once per run rather than per tool. Generated output is byte-identical and `agentsync check` still passes; an 11-tool sync runs roughly 1.6× faster.

## 0.27.1

### Fixed

- **`--help` on argument-less subcommands:** `agentsync check|setup-hooks|doctor|list|show|diff|disable|resolve --help` now prints the top-level usage instead of mis-reading `-h`/`--help` as a positional argument.
- **`copy_rules` refuses to delete an empty destination:** rule sync now aborts with a clear error instead of running `rm -rf` against an empty `dest_dir`, guarding against wiping an unintended directory.

### Changed

- **Drift-abort message points to `adopt`:** when `sync` aborts because a generated file was changed out of band (e.g. a plugin writing into `.claude/settings.json`), the guidance now suggests `agentsync adopt <file>` to pull the change into `.ai/src/`, alongside the existing move-to-source and `--force` options.

## 0.27.0

### Added

- **`paths:`-scoped rules now translate to every tool's native trigger:** a rule that declares `paths:` frontmatter (a list of globs) is emitted with each tool's _scoped_ trigger instead of the always-on header — Cursor `globs` + `alwaysApply: false`, Copilot `applyTo`, Windsurf/Antigravity `trigger: glob`, while Claude keeps `paths:` verbatim — so a domain rule (state, routing, data…) loads only when matching files are touched, keeping the always-on context lean. Configured via the new `targets.rules.scoped_header` option (a header string with a `{globs}` placeholder); rules without `paths:` still get the always-on `header`. Tools that inline rules into their agents file now skip a rule's frontmatter when building the index, so a scoped rule shows its heading rather than the `---` delimiter.
- **`doctor` flags always-on rule bloat:** a new advisory under a "Rules" section warns when the always-on rule set (rules without `paths:`) grows past ~20 KB / ~5k tokens, since a large always-on set dilutes attention and agents start ignoring individual instructions. Advisory only — like other techdebt detections it prints `⚠` but never affects the exit code.

## 0.26.3

### Changed

- **`agentsync add mcp` merges `mcp.json` without python3:** the shared MCP source is now edited in pure Bash + awk instead of an embedded python3 script. Previously the command hard-exited with "python3 is required to edit mcp.json safely" on any machine without python3 — contradicting AgentSync's zero-runtime-dependency guarantee. The new merge is string-, escape-, and brace-depth-aware, so it preserves existing servers (including arbitrary nesting) and JSON-escapes values correctly. Server entries are now written one per line (compact JSON values); an existing `mcp.json` reflows to that shape the next time you run `add mcp`.

### Fixed

- **Docs match the shipped behavior:** corrected the `agentsync adopt` refused-target lists (header tools are cursor/copilot/windsurf/antigravity, only zed merges rules into one file, and the inline set is codex/gemini/junie for rules plus amazonq/cline/zed for skills), removed a reference to a non-existent `sync --cleanup` flag, and fixed the `agentsync update` description (it runs a background check on each interactive run, not a 24h timer). Surfaced `--profile` in `sync --help` and the workspace forwarded-options list. Added a Profiles section to the README and refreshed the bundled `agentsync` skill — and its shipped template — for current tool coverage, `add mcp`, and the per-tool override layout.

## 0.26.2

### Fixed

- **Windows: text files check out with LF so frontmatter parsing works:** a `.gitattributes` (`* text=auto eol=lf`) now pins LF line endings on every platform. On Windows (Git Bash), `core.autocrlf` checked text files out as CRLF, and the frontmatter parsers match `/^---$/` exactly — a `---\r` delimiter never matched, so description extraction returned empty and broke the command-rendering paths (Codex `command-*` skills, Amazon Q / Zed inline "## Commands" sections). Scripts and the `.md` templates/fixtures the parsers read now check out LF identically on macOS, Linux, and Windows.

## 0.26.1

### Fixed

- **`agentsync update` self-heals local install-dir drift instead of failing:** when the global install (`~/.agentsync`) had a local edit to a tracked file that an incoming release also touched, the underlying `git pull` aborted ("Your local changes would be overwritten by merge") and the real git error was hidden — you saw only a generic "git pull failed, try reinstalling" message. `update` now reconciles the install dir toward the release: local edits to tracked files are set aside into a recoverable stash (`git stash list`), the update fast-forwards to the fetched release, and a diverged history is reset onto it. Untracked files (`.update_cache`) and the conflict snapshot (`.snapshot/`) are left intact, and the run reports when edits were set aside. The install dir mirrors a release, not a working branch, so reconciling toward it is the intended behavior.

## 0.26.0

### Changed

- **Sync preserves hand-placed files in generated dirs:** `agentsync sync` no longer treats a tool's output directory as fully sync-owned. Previously, a file you added by hand to a generated dir (e.g. `.claude/rules/my-own.md`) was silently deleted on the next sync because it wasn't regenerated from `.ai/src/`. Sync now consults the manifest: an extraneous entry it never generated is kept — with a per-file warning and a run-summary tally ("Preserved N user-added file(s)…") — instead of removed. The file is still _unmanaged_; move it into `.ai/src/` to manage it, or run `agentsync sync --force` to restore the old prune-everything behavior. Manifest-unaware callers keep their previous semantics, so only real sync runs change.

### Fixed

- **`profile add` scaffolds a README instead of empty overlay dirs; `--adopt` dereferences symlinks:** `agentsync profile add` no longer pre-creates empty `rules/`/`skills/` overlay directories (clutter git can't track anyway) — it writes a self-documenting README at the overlay root and creates content dirs on demand. With `--adopt`, an existing `~/.<tool>-<name>/` directory is now copied with `cp -RL`, so symlinked plugin skills are dereferenced into real files and broken links are skipped rather than copied dangling.
- **`snapshot_save` rejects empty target directories:** an empty `snapshot_dir` made the internal cleanup target a bare `/tools` path. Both `install_dir` and `snapshot_dir` are now validated and the function returns early on either being empty.

## 0.25.0

### Added

- **List form for `include` / `exclude` filters:** `targets.<resource>.include` and `targets.<resource>.exclude` now accept a YAML list — block style (one `- glob` per line) or inline `[a, b]` — in addition to the original space-separated scalar. Long filter lists (e.g. preserving plugin-managed skills) become one glob per line instead of a single wide string. Read via the new layered `get_tool_filter` resolver; existing scalar configs are unchanged.

## 0.24.0

### Added

- **`agentsync profile` — config-home profiles for per-account tool variants:** a profile fans one source tree out into a second, self-contained config-home directory for the same tool — e.g. a work `~/.claude-hub/` (run with `CLAUDE_CONFIG_DIR=~/.claude-hub`) alongside your personal `~/.claude/` — each with its own content. `agentsync profile add <name> [--tools a,b] [--adopt]` scaffolds, per tool, a thin variant config `.ai/src/tools/<tool>-<name>.yaml` (declaring `base: <tool>` to inherit every unset field, with config-home `targets.*.dest`), an overlay dir `.ai/profiles/<name>/src/` for profile-only rules/skills/commands/agents, and a `profiles:` block in `agent_sync.yaml`. `agentsync profile list` and `agentsync profile remove <name>` round out the lifecycle. Use `--adopt` to pull an existing `~/.<tool>-<name>/` directory into the overlay before the first sync.
- **`agentsync sync --profile <name>`:** sync personal tools plus the named profile; a plain `agentsync sync` also syncs every profile marked `active: true`. Each profile renders with a per-profile source overlay (`.ai/src/` base ⊕ `.ai/profiles/<name>/src/`, profile wins on path conflicts), composing on top of an active `shared:` overlay. Profile outputs are gitignored and drift-protected like any other output.
- **`base:` tool field:** a tool config may declare `base: <tool>` to inherit every field it does not set (formats, extensions, inline flags) and the base tool's `settings`/`mcp`/`hooks` templates from another tool. This backs profile variants but is available to any custom tool that wants to extend a shipped one.

## 0.23.3

### Fixed

- **`sync` and `init` refuse to run from inside the `.ai/` source directory:** running either command while the working directory was inside `.ai/` (e.g. `cd project/.ai && agentsync sync`) rooted the engine at the source tree, so `sync` wrote tool outputs _under_ `.ai/` and `init` created a nested `.ai/.ai/` — instead of generating at the project level alongside `.ai/`. Both commands now detect this, stop with exit code `2`, and point you at the project root (the parent of `.ai/`): `cd "<project>" && agentsync sync`. The new `ai_dir_enclosing_root` helper in `lib/helpers/paths.sh` backs the guard. No change for runs started from the project root.

## 0.23.2

### Added

- **Git rule and commit skill now forbid AI-attribution trailers:** `lib/templates/rules/git.md` and `lib/templates/skills/commit/SKILL.md` (and this project's own `.ai/src/` copies) gain an explicit instruction barring `Co-Authored-By:`, `Generated with …`, and tool/model signatures from commit messages — a commit records the human author only. The Claude `settings.json` template already sets `includeCoAuthoredBy: false`, but that knob is Claude-specific; the other ten tools have no equivalent, so the prohibition lives in the synced rules where every tool reads it. Existing projects pick it up via `agentsync refresh`; new projects get it on `init`. No change to the sync engine.

## 0.23.1

### Fixed

- **Templates no longer ship an invalid `model: "default"` pin (regression from 0.23.0):** 0.23.0 replaced the hardcoded `model: sonnet`/`opus` in the Claude `settings.json` template and subagent scaffolds with the literal `model: "default"`. Claude Code reads that literal as a nonexistent custom model — it surfaces as a "Custom model" in `/model` and errors on operations like `/compact` ("the selected model (default) may not exist"). The correct way to express "use the recommended default model" is to **omit** the `model` field entirely (the `/model` "Default" option clears the key rather than writing a value). 0.23.1 drops the `model` field from `lib/templates/settings/claude.json`, `lib/templates/agents/code-reviewer.md`, `lib/templates/content/subagent.md`, and the `agentsync generate` prompt, so scaffolds inherit the account default. Existing projects: run `agentsync refresh` to pick up the corrected templates, and remove any `"model": "default"` line a 0.23.0 scaffold wrote into `.claude/settings.json` or a subagent's frontmatter.

## 0.23.0

### Changed

- **Shipped templates stop pinning a model in scaffolds:** the Claude `settings.json` template (`lib/templates/settings/claude.json`) and the subagent scaffolds (`lib/templates/agents/code-reviewer.md`, `lib/templates/content/subagent.md`, and the `agentsync generate` prompt) drop the hardcoded `model: sonnet`/`opus` so new projects inherit the account's recommended model instead of a pinned snapshot. (0.23.0 set the field to `model: "default"`, which Claude Code rejects as an unknown model; corrected in 0.23.1 to omit the field.) Projects that deliberately pin a model through a `.ai/src/tools/<tool>/` override are unaffected; the skill's cost-saving guidance to pin `sonnet`/`haiku` for focused subagents still stands as an opt-in.

- **`prompt-engineering` skill updated for Claude Opus 4.8:** the tool-specific notes now treat Opus 4.8 as the current most-capable GA model — it builds on 4.7 with no breaking API changes (the carried-over behavioral notes still apply), `effort` defaults to `high` (set `xhigh` for coding), the 1M context window is served by default, and mid-conversation `role: "system"` messages are accepted. The pinned-snapshot example moves from `claude-opus-4-7` to `claude-opus-4-8`, and carried-over behavior labels shift from `4.7` to `4.7+`. Surfaces on existing projects via `agentsync refresh`; new projects get it on `init`. No change to the sync engine.

- **README and bundled best-practices reference brought back in line with the engine:** the README "Supported Tools", "Format Conversions", "Key Fields", and CLI command tables now match the actual `lib/templates/tools/*.yaml` configs and `bin/agentsync.sh` — corrected Junie (`.junie/AGENTS.md` + inlined rules, skills dir), Cursor (`commands`), Windsurf (`workflows`, `hooks`), Amazon Q (`mcp`, `cli-agents` MD→JSON), Gemini/Codex/Zed (`settings`) targets; documented the `amazonq_json` subagent converter; added the 12 previously-undocumented commands (`enable`, `disable`, `add`, `customize`, `simplify`, `show`, `diff`, `resolve`, `export`, `import`, `upgrade-config`, `release`); and fixed the tool count to 11. The vendored `knowledge/best-practices.md` is on the Claude Opus 4.8 guidance.

## 0.22.0

### Changed

- **Google Antigravity output moved from `.agent/` to `.agents/`:** the Antigravity CLI's workspace output directory changed from `.agent/` (singular) to `.agents/` (plural) to match its current plugin convention. `lib/templates/tools/antigravity.yaml` and the example project now write `rules`, `skills`, and `commands` under `.agents/`. The `agents` target stays at the canonical root `GEMINI.md`. Existing projects pick up the new layout on the next `agentsync sync`; the old `.agent/` directory is now treated as pre-v0.6 legacy and surfaced by `agentsync doctor` / `migrate` for cleanup.

- **`doctor` and `migrate` no longer protect `.agent/` when antigravity is enabled:** the dual-purpose guard added in 0.20.2 (treat `.agent/` as live Antigravity output when antigravity is in `tools.enabled`) is removed. With Antigravity's path now `.agents/`, the guard would shield stale `.agent/` directories from cleanup — the opposite of what the user wants on a transitional project. `.agent/` is once again pure pre-v0.6 legacy regardless of tool enablement; `doctor` advises on it and `migrate --apply --yes` removes it. The shared-`.agents/` orphan check in doctor was extended to skip when antigravity is enabled (it already skipped for codex).

## 0.21.0

### Added

- **`targets.commands.as_skills`:** new per-tool YAML option that emits each `.ai/src/commands/<name>.md` as a generated skill at `<targets.skills.dest>/command-<name>/SKILL.md` for tools without a native slash-command surface. Codex CLI is the headline beneficiary — its built-in slash commands are hardcoded and OpenAI explicitly recommends skills as the replacement, so AgentSync's project-local commands now reach Codex through `.agents/skills/command-*/` automatically. Generated skills regenerate frontmatter (`name: command-<name>`, description carried over) and rewrite Claude-flavoured slash sugar (`$ARGUMENTS` → `<arg>`, leading `` !` `` → `` ` ``) into prose the skill reader can use. The `command-` prefix avoids collisions with native skills of the same name — `release` and `command-release` coexist as separate dirs. Enabled by default in the base template for Codex; toggle with `agentsync customize codex` and override `commands: { as_skills: false }`.

- **`targets.commands.inline_into_agents`:** parallel option for tools that lack BOTH a native commands surface AND a skills dir (Amazon Q, Zed). Appends a `## Commands` index to the agents-like file (or to the merged rules file when `rules.merge_to_file: true`) with one ``- `/<name>` — <description>`` line per command. Enabled by default for Amazon Q (`.amazonq/rules/00-context.md`) and Zed (`.rules`).

- **`agentsync doctor` per-tool config conflict checks:** doctor now warns when a tool mixes `targets.commands.dest`, `.as_skills`, and `.inline_into_agents` — only one wins in sync, the others are silently ignored. Also warns when `as_skills: true` is set without `targets.skills.dest`, or `inline_into_agents: true` without `targets.agents.dest`, since both options would no-op.

- **`agentsync sync` info line for fallback modes:** when sync emits commands as skills or inlines them into AGENTS.md (rather than copying to a native commands dir), it now prints a one-line explanation so first-time users understand why their `.ai/src/commands/` lands in an unexpected destination.

### Changed

- **`matches_filter` now accepts space-separated patterns:** `include` and `exclude` in YAML can list multiple globs separated by spaces (e.g. `exclude: "draft-*.md tmp-*.md"`); the filter matches if ANY pattern matches. Single-pattern usage is unchanged.

- **`sync_dir` cleanup honors `exclude`:** files in the destination directory that match the caller's `exclude` glob are no longer swept as extraneous. This lets a second sync step (e.g. `sync_commands_as_skills`) safely own a subset of the destination without the primary `sync_dir` call deleting its output on each run.

- **`--help` / `-h` flag now works on `agentsync customize`, `enable`, `add`, `show`, `diff`:** previously these subcommands rejected `--help` as "Unknown flag". Each now prints a usage block with flags, positional args, and a one-line summary of what the command does.

## 0.20.4

### Fixed

- **`agentsync init` and `refresh` now copy nested skill subdirectories:** previously `init` shallow-copied each skill via `cp "$skill_dir"*` (no `-R`), and the `find` walks under `skills/` in `refresh`, `dedupe`, `doctor`, and the template-manifest healer were limited to `*.md` / `*.markdown`. Skills that ship companion content under `references/` or `scripts/` (e.g. `prompt-engineering/references/agent-persona.md`, `humanizer/references/wikipedia_signs_of_ai_writing.md`) were silently dropped on `init` and never picked up by `refresh` — the user got a `SKILL.md` that linked to files which weren't on disk. The `init` copy now iterates entries and uses `cp -R` so subdirectory layouts survive, and the four `find` walks now match every non-hidden file under `skills/` (`! -name '.*'`) instead of just markdown. `refresh` will now offer the missing nested files as NEW on existing projects; new projects get the full skill tree on `init` automatically.

## 0.20.3

### Changed

- **Humanizer skill expanded:** added a "Matching a sample" section so the skill calibrates to a user-provided writing sample when one is supplied (matching sentence-length patterns, paragraph openings, recurring word choices, and punctuation habits instead of defaulting to the skill's house voice). New pattern entries cover tailing negation fragments (", no guessing," ", no wasted motion," tacked onto a sentence end), persuasive authority tropes ("The real question is," "At its core," "Fundamentally"), signposting and self-narration ("Let's dive in," "Here's what you need to know"), elegant variation (synonym cycling for the same noun within a passage), and fragmented headers (a heading followed by a one-line restatement of the heading before the real content begins). The final-pass checklist gains a fourth meta-audit step that asks the model to name remaining AI tells in the draft — too-symmetrical rhythm, slogan-y closers, placeholder-sounding names — and rewrite those specific spots. Surfaces on existing projects via `agentsync refresh`; new projects pick up the refined content automatically on `agentsync init`. No behavioural change in the sync engine.

## 0.20.2

### Fixed

- **`agentsync dedupe` now honors `shared.path` symmetric with `doctor` 0.20.1:** in a monorepo where some sub-projects have their own `.git`, `dedupe` without `--against` silently skipped them because git-bounded walk-up stopped at their boundary. The `--workspace` mode doesn't accept per-project `--against`, so the workaround was a manual `cd subproj && dedupe --against ../` fan-out that defeats the point of `--workspace`. After 0.20.1, users could rely on `doctor` to catch governance drift across the whole workspace, but their natural next step — clean it up with `dedupe` — fell back into the asymmetry that `doctor` had just escaped. `dedupe` now resolves the parent via `shared.path` first (matching how the sync-time overlay and `doctor` already cross repo boundaries), and falls back to git-bounded walk-up only when `shared:` isn't declared. `--against` keeps winning over both. The "Parent:" line gains a `(from shared.path)` hint when the override took effect, mirroring `doctor`'s output. Applies to both single-project and `--workspace` modes — `--workspace` in particular now correctly dedupes every sub-project that declared `shared.path` regardless of git topology.

- **`agentsync doctor` no longer flags `.agent/` as legacy when Google Antigravity is enabled:** doctor's `Tool outputs` section unconditionally treated `.agent/` (singular) as the pre-v0.6 monolithic layout that needed cleanup. But `.agent/` is also Google Antigravity's current canonical output directory — same path, completely different meaning. With antigravity enabled, the false-positive advisory pollutes `OK with N advisory(ies)` summaries and conditions users to ignore the entire section. The check now skips when `antigravity` is in `tools.enabled`; without it, the legacy detection works as before.

- **`agentsync migrate` no longer destroys Google Antigravity output:** the same `.agent/` collision had a more dangerous form on the migrate side — `migrate --apply --yes` would delete the directory because `_migrate_has_legacy_agent_dir` returned true regardless of tool enablement. A user running migrate in a script after enabling Antigravity would erase live tool output silently. Migrate now applies the same enablement check as doctor: with antigravity enabled, `.agent/` is treated as managed output and left alone; with antigravity disabled, the existing legacy detection and cleanup paths still run. As a corollary, `_migrate_prepare_context` now resolves `PROJECT_CONFIG_PATH` (the same way `_doctor_prepare_context` does) so the enablement check can actually read the project config — previously migrate was missing this setup entirely, which silently broke any helper that depended on it.

## 0.20.1

### Fixed

- **`agentsync doctor` now honors `shared.path` as a cross-project parent override:** in a workspace where some sub-projects share `.git` with the parent and others have their own `.git`, doctor previously flagged governance divergence in the former but stayed silent in the latter — even though both opted into the same `shared:` declaration. Walk-up correctly stops at the git boundary as an auto-detection safety guard, but it shouldn't override an explicit user declaration. Doctor now resolves the parent via `shared.path` first (matching how the sync-time overlay already crosses repo boundaries), and falls back to git-bounded walk-up only when no `shared:` is declared. The "Parent source:" line gains a `(from shared.path)` hint when the override took effect, so it's clear which mechanism resolved the parent. Existing behaviour without `shared:` is unchanged — the git-boundary guard still prevents doctor from comparing unrelated repos during auto-detection. `agentsync dedupe` was not affected by the bug because it already exposes `--against PATH` as an explicit escape hatch.

## 0.20.0

### Added

- **`agentsync dedupe` — interactive cross-project duplicate cleanup:** new command that compares this project's `.ai/src/` against a parent project's `.ai/src/` (auto-detected by walking up to the first parent containing one, bounded by the git repository boundary so it never escapes the current repo). For each shared path it shows either an identical-hash duplicate (offer to delete from this project) or a divergent file (show diff and let the human decide — dedupe never auto-resolves a divergence). For files that originated as shipped templates, the deletion is paired with a `template_overrides.declined` entry so `refresh` won't re-offer the file. Manual duplicates that aren't shipped templates are deleted without a manifest entry — declined is a template-only mechanism, and writing dangling entries for non-template files would break refresh's lookup semantics. Three modes: default (compare against the walked-up parent), `--against PATH` (compare against an arbitrary `.ai/src/` or project root), and `--workspace` (bottom-up alphabetical fan-out across every `.ai/` below cwd; each child is deduped against its own nearest parent). Non-interactive use via `--yes` deletes identical-hash files but never picks a side on divergent ones. Empty parent skill directories left behind after deletion are pruned automatically so the tree doesn't accumulate empty `skills/<name>/` shells after their `SKILL.md` is removed.

- **`agentsync sync --workspace` — recursive fan-out across nested projects:** new flag that runs `sync` in every `.ai/` below the current directory, in bottom-up alphabetical order (deeper paths first, siblings sorted by `LC_ALL=C` for reproducible output across runs and machines). Continue-on-failure: a broken sub-project doesn't abort the loop; the run reports the max exit code at the end. All other sync options (`--only`, `--skip`, `--dry-run`, `--force`) forward to each per-project invocation. Replaces the manual `cd msd && sync && cd ../resume && sync && ...` shell loops that workspace users had been writing by hand. A bare `.ai/` directory without `src/` or `agent_sync.yaml` is skipped — only AgentSync-managed trees are picked up, so unrelated `.ai/` directories from other tooling don't pollute the loop.

- **`agentsync doctor` — cross-project, orphan-output, and empty-skill detection:** doctor gains three new sections. **Cross-project** compares the project's source files against a walked-up parent `.ai/src/` (same walk-up as `dedupe`, bounded by git repository), flagging identical-hash duplicates as advisories and divergent files as info — `category: governance` files (see below) are upgraded from info to advisory with explicit "likely a mistake, not an override" framing. **Tool outputs** flags any `.claude/`, `.cursor/`, `.codex/`, etc. directory whose owning tool isn't enabled in this project (orphan from a prior run after the tool was disabled, or a leftover from a different project copy-pasted into the tree), plus the legacy `.agent/` (singular) layout from before v0.6 — that directory was never cleaned up by `sync` because the current engine doesn't know it exists. **Skills** flags skill directories that lack a `SKILL.md` (a no-op artifact that lists nowhere and dispatches nothing). All three detections use a new **advisory tier** that displays as a yellow ⚠ but does not increment `DOCTOR_WARNINGS` and does not change the exit code — so users who run `doctor` in pre-commit hooks or CI pipelines get the techdebt nudge without breaking the build. Hard errors (missing `.ai/`, invalid YAML) still exit 2; warnings (legacy `enabled: true`, version drift, manual edits since last sync) still exit 1; the new techdebt category is exit-0 by design.

- **`shared:` resources — declarative inheritance from a parent `.ai/src/`:** new optional section in `agent_sync.yaml` that lets a nested project pull source files from its parent at sync time. Syntax is intentionally minimal — no YAML parser changes — using inline CSV for the inherit list:

  ```yaml
  shared:
    path: "../"
    inherit: rules,skills,commands,agents
  ```

  At sync, AgentSync builds a transient shadow `.ai/src/` tree under a temp directory: child's own files first, then parent files in inherited categories filling in any path the child doesn't already have (child wins on path collisions — overlay never overwrites local content). Sync then reads from the shadow tree so every enabled tool — including ones without parent-loading (Codex, Cursor, JetBrains Junie, etc.) — receives the inherited content materialized into its own output. The shadow tree is read-only from the child's perspective and is torn down via an EXIT trap, so there's no on-disk state to manage. **Manifest interaction is deliberately one-sided:** `.template-manifest` continues to track only files actually present in the child's `.ai/src/`; inherited files are not in the manifest and are never offered by `refresh` — they belong to the parent. This is the design point that came out of the v1 → v2 architectural review: a single state mechanism, no "inherited" flag bag, no second source of truth that can drift out of sync with the first. When `doctor` finds a same-path duplicate in an inherited category, it appends `(inherited via shared: — safe to delete)` to nudge users toward deletion — once deleted, parent will continue to provide the content via overlay on every subsequent sync.

- **`category:` frontmatter field — declaration of intent for rules and skills:** optional `category:` in any YAML frontmatter (rules, skills, commands, agents). Today only `governance` carries behavior — when `doctor` finds a child file that diverges from its parent's same-path version and the parent file has `category: governance`, the message escalates from info ("review intent") to advisory ("governance file diverges from parent — likely a mistake, not an override"). Other category values (`domain`, `workspace`, `project`) are accepted and recorded but currently informational; future tooling can build on them without forcing existing projects to migrate. Designed so adding a category to an existing template is a non-breaking, opt-in declaration.

- **`agentsync refresh --status` — declined breakdown:** new flag that prints the full list of declined templates and exits, split into **Persistent** (entries in `template_overrides.declined` in `agent_sync.yaml`) and **Local** (entries in `.template-manifest` whose file is missing on disk). Useful for users with many accumulated overrides who want to see what's declined without scanning the YAML by hand or running `--include-deleted` (which has different semantics — it re-offers the files for restoration). Returns 0 with no output if nothing is declined.

- **`agentsync migrate` — legacy `.agent/` (singular) detection and cleanup:** migrate now recognizes the pre-v0.6 monolithic `.agent/` layout (a single directory holding `AGENTS.md`, `workflows/`, `rules/`, `skills/` without per-tool separation). The current engine doesn't know about it, so `sync --cleanup` never sweeps it. Dry-run lists the contents so the user can confirm before removal. `--apply --yes` removes the directory; `--apply` without `--yes` on a non-TTY leaves it in place with a hint (conservative: never silently remove user content). Detection runs alongside the existing flat-layout `.ai/src/{hooks,mcp,settings}/` move logic, so a single `migrate --apply --yes` cleans up both in one pass.

### Changed

- **`agentsync refresh` up-to-date output splits declined sources:** when the run is a no-op, the summary now reports `Persistently declined (agent_sync.yaml): N file(s)` and `Locally declined (.template-manifest): M file(s)` as separate lines, and appends `Pass --status for the full list.` when either count is non-zero. Replaces the single ambiguous `N file(s) previously declined` line from 0.16.0 that conflated the two sources — relevant for users who have accumulated entries in both the persistent override list (which `--include-deleted` ignores) and the local manifest (which `--include-deleted` revisits).

- **`agentsync doctor` exit-code semantics — new advisory tier never breaks CI:** doctor's existing errors (exit 2) and warnings (exit 1) keep their current behavior. The new techdebt detections (cross-project duplicates, orphan tool outputs, empty skills, legacy `.agent/`) live in a third tier that prints as a yellow ⚠ but does not increment `DOCTOR_WARNINGS` and does not affect the exit code. The summary line now reports `OK with N advisory(ies)` when only advisories were found. Lets users wire `doctor` into pre-commit / CI gates without legacy techdebt forcing a fail-on-warning policy to be relaxed — the cleanup nudges are visible during interactive runs but invisible to automation.

## 0.19.0

### Removed

- **Aider, Augment Code, and Continue support dropped:** the three lowest-traffic tool integrations are gone — `lib/templates/tools/{aider,augment,continue}.yaml`, the matching `lib/templates/settings/{aider,continue}.yaml` base settings, the auto-detect markers in `agentsync init`, the README tool table rows, the conversion-matrix rows, the knowledge index sections, and the bats assertions all removed. Rationale: market data through April–May 2026 shows Aider sitting on a small terminal-only niche, Continue having pivoted away from rules-as-files toward CI checks (so its sync surface no longer matches the AgentSync model), and Augment serving an enterprise-only compliance audience that doesn't bootstrap via `curl | bash`. Maintaining all three was costing ~15–20% of the per-release tool-config surface for a sliver of real users. Existing projects with `aider`, `augment`, or `continue` entries in `.ai/src/tools/` will see those tools silently skipped on the next `sync` — no error, just no output written. Users who still want one of these tools can keep their tool YAML in `.ai/src/tools/` (the sync engine is config-driven and will continue to honour a project-local file), or copy the last shipped version from git history (`git show 0.18.0:lib/templates/tools/aider.yaml`) into their own project as a custom integration.

### Changed

- **Rule and skill templates rewritten for clarity:** `lib/templates/AGENTS.md`, `lib/templates/rules/{comments,core,git}.md`, and most of `lib/templates/skills/*/SKILL.md` (agentsync, comments, commit, debug, humanizer, prompt-engineering, refactor, review) trimmed and sharpened — same constraints, shorter lines, tighter examples, fewer redundant "what to avoid" lists collapsed into single positive-form statements. Surfaces via `agentsync refresh` on existing projects (the manifest will offer the updated files); new projects pick up the refined content automatically on `agentsync init`. No behavioural change in the sync engine.

## 0.18.0

### Changed

- **`agentsync refresh` — `[s]kip` on a conflict is now remembered:** 0.16.0 left `[s]kip` on a CONFLICT deliberately unrecorded, so every subsequent refresh re-prompted for the same file until the user resolved or pinned it. In practice, that meant a tree with a handful of intentionally diverged rules surfaced the same wall of conflicts on every refresh — friction that pushed users toward editing `template_overrides.pinned` by hand to silence the noise. `[s]kip` now records the current template hash in `.ai/.template-manifest`, so the divergence stays silent on future refreshes until a newer template ships (at which point the conflict resurfaces automatically against the new content — you never lose visibility of a real update). The post-skip confirmation reads `skipped (remembered — agentsync refresh --review to revisit)` so the new behavior is discoverable from the prompt itself. `template_overrides.pinned` in `agent_sync.yaml` remains the stronger, unconditional pin (never resurfaces, even when the template moves) — `[s]kip` is now the lightweight "remember until something changes" choice that handles the common case.

### Added

- **`agentsync refresh --review` — revisit remembered skips and local edits:** new flag that resurfaces every local divergence from the shipped templates, including conflicts you previously `[s]kipped` and files you've edited locally since the last refresh. Use it to revisit earlier decisions ("did I really mean to skip that?"), audit local edits against the upstream templates, or sweep through divergences in batch. `template_overrides.pinned` still wins — pinned files stay silent even under `--review`, so the YAML override remains the definitive way to opt out forever. Pairs with the new "skip is remembered" behavior: on a normal refresh, silently-kept files don't clutter the summary; on a `--review` refresh, they're listed as conflicts with the same `[u]pdate / [s]kip / [v]iew / [q]uit` prompts. Under `--yes`, surfaced conflicts are reported but never overwritten — keeps the existing CI-safety guarantee that `--yes` never makes a destructive choice on a divergence.
- **`agentsync refresh` summary hint when files differ silently:** the summary block (and the `Already up to date!` no-op message) now appends `· N silently kept (local edits or earlier skips) — pass --review to revisit` whenever the classifier finds files that match the manifest baseline but diverge from the template. Makes the new flag discoverable without forcing users to read `--help`, and quietly nudges users who have accumulated local edits to do an audit pass.

## 0.17.0

### Added

- **`humanizer` skill bundled with init templates:** `lib/templates/skills/humanizer/SKILL.md` plus a `references/wikipedia_signs_of_ai_writing.md` reference and a deterministic `scripts/strip-ai-chars.sh` cleanup script now ship with `agentsync init`, so freshly bootstrapped projects inherit a ready-to-use skill for rewriting AI-flavored prose (and for generating long-form deliverables — blog posts, essays, newsletters, op-eds, short stories — in the same plain, grounded voice from the first draft). The skill triggers on humanize / de-AI / de-slop requests in any language, on prose-deliverable asks, and explicitly skips code, casual chat, and technical documentation. Guidance is language-agnostic: instead of an English-only banned-word list, the skill teaches the underlying principle (swap any word that sounds inflated, abstract, or official compared to how a person would actually say the thing for the plain everyday equivalent) and lets the model apply it in whatever language the piece is in. Detection criteria cover punctuation tells (em/en dashes, semicolons, framing colons, mid-paragraph bold), inflated vocabulary (utilize, leverage, delve, tapestry, robust, multifaceted, etc.), structural tics (forced triads, contrastive negation, pivot transitions, significance inflation, empty intensifiers, metaphor verbs, fiction tropes, sycophantic openers, chatbot closers), and rhythm/format issues (monotone medium-length sentences, bullets that should be prose). The `references/wikipedia_signs_of_ai_writing.md` companion is a condensed reference distilled from Wikipedia's "Signs of AI writing" so the skill stays under its primary context budget while keeping the full detection catalog one read away.
- **`scripts/strip-ai-chars.sh` — deterministic typographic cleanup:** ported from the MIT-licensed [humanize-ai-lib](https://github.com/Nordth/humanize-ai-lib), this `perl -CSDA -pe`-driven filter takes text on stdin and writes a cleaned version to stdout. It strips zero-width and bidi-control watermarks (the invisible characters often used to fingerprint AI output), C0/C1 controls, tag-character watermarks (U+E0000–E007F), and decorative Unicode blocks no keyboard produces (math alphanumerics, arrows, math operators, box/block drawing, enclosed alphanumerics, dingbat bullets). It also normalizes the visible-but-AI-tell punctuation that the prose pass would otherwise have to catch by ear: non-breaking spaces collapse to regular spaces, en/em dashes become hyphens, curly/guillemet quotes become straight quotes, smart apostrophes become straight apostrophes, ellipsis characters become three dots, and trailing whitespace is trimmed. Idempotent and dependency-free beyond Perl (preinstalled on every macOS / Linux / Git Bash environment AgentSync supports), so the skill's prose pass can focus on rhythm and word choice while the script handles the deterministic character-level cleanup.

## 0.16.0

### Added

- **`agentsync refresh` — three-way diff via `.ai/.template-manifest`:** the manifest is a content-addressed record (one `<rel-path>\t<sha256>` line per file, LC_ALL=C sorted) of every template file copied into `.ai/src/` via `init` or accepted via `refresh`. `init` now writes a baseline on every fresh scaffold; `refresh` reads it and routes each template through three-way diff (template-old vs template-new vs user-current), so the file-classification graph becomes much richer than 0.15.0's two-way (NEW / CONFLICT / UNCHANGED). New states: **AUTO_UPDATE** — user untouched + template moved → applied silently without a prompt (the big UX win for users who are several CLI versions behind: a project with 12 unchanged-by-user templates and 4 user-edited ones now sees 12 silent auto-updates and only 4 interactive conflicts, instead of 16 indistinguishable conflict prompts); **USER_EDITED_NO_CHANGE** — user edited locally + template static → silent (their custom version stays, no nag); **DELETED** — file removed locally that was once scaffolded → treated as an intentional decline and silent (pass `--include-deleted` to revisit). Skipping a NEW prompt now records a manifest entry, so a declined-by-skip template isn't offered again on subsequent refreshes — pass `--include-deleted` to revisit them. Skipping a CONFLICT deliberately does **not** record, preserving the "unresolved divergence" signal until the user accepts or pins it. Backward compatibility is preserved: projects without a manifest fall back to two-way diff (every divergence is a CONFLICT, `--yes` skips), and the first refresh on such a project heals the manifest from currently-matching files so subsequent refreshes get full three-way semantics without forcing a wall of accepts. Commit `.ai/.template-manifest` to git so the team shares the same baseline; otherwise different developers will see different conflict sets.
- **`template_overrides` in `agent_sync.yaml` — persistent skip and pin:** two new optional lists silence specific templates forever. `template_overrides.declined: [rel/path/to.md, ...]` makes refresh ignore those templates entirely (never surfaced, never offered, even with `--include-deleted`). `template_overrides.pinned: [rel/path/to.md, ...]` accepts that the user maintains a divergent version and suppresses the conflict prompt on those files (template moves silently, user's version stays). Read-only in this revision — users edit the YAML directly to mark overrides; an interactive write-from-prompt is deferred to a future release. Closes the UX gap where every refresh kept asking about the same skipped files.
- **`agentsync refresh --include-deleted`**: surfaces files the user removed locally so they can be restored. Lists them in the dry-run plan and offers per-file `[a]dd / [s]kip / [v]iew / [q]uit` prompts in interactive mode. Under `--yes` they remain skipped — restoring a previously-declined file is a deliberate choice that should not happen by default in a non-interactive run. Lets users recover from accidental `rm` or revisit a template they earlier dismissed without remembering exactly which one.
- **`lib/helpers/template_manifest.sh`**: new module mirroring `lib/helpers/manifest.sh`'s shape (parallel `TEMPLATE_MANIFEST_KEYS` / `TEMPLATE_MANIFEST_VALUES` arrays, sha256 via `sha256sum` or `shasum -a 256`, atomic write via temp + `mv`, `LC_ALL=C sort -u` for byte-stable output). Pure bash 3.2, zero external deps beyond coreutils. Exposes `template_manifest_load` / `_lookup` / `_record` / `_write` plus `template_manifest_heal_from_match` — the helper that `init` and `refresh` both use to populate manifest entries for files that already match the current template (lets v0.15.0-era projects upgrade to three-way diff without re-accepting every file).

### Changed

- **`agentsync init` writes `.ai/.template-manifest`**: after scaffolding source content, init now records the template hash for every file it copied. This is the baseline that `refresh` needs to do three-way diff. The behavior is fully backward compatible — pre-0.16 projects without a manifest get two-way diff on their first refresh (with `--yes` skipping conflicts), then the manifest is healed from matches and subsequent refreshes get three-way semantics.

## 0.15.0

### Added

- **`agentsync refresh` — pull template updates into an existing `.ai/src/`:** new command that walks the shipped templates (rules, skills, commands, agents) and compares each file against the project's `.ai/src/`. New templates are offered for adding; modified files show a unified diff so the user can update or skip per file. Files in `.ai/src/` that aren't part of the templates (the user's custom rules/skills) are left alone. Closes the long-standing gap where users had no path to inherit template additions (e.g. the `comments` rule/skill from 0.14.0) or rewrites (e.g. the positive-form rewrite from 0.13.0) without re-running `init` from scratch — `init` refuses on existing trees by design, and `import` only knows how to pull from external bundles, not from the locally installed templates. Behavior is safety-first throughout: default action on Enter is **skip** (never auto-accept a conflict), `--yes` adds new files but skips conflicts (CI-friendly; conflicts must be reviewed manually), non-TTY without `--yes` errors with a hint, and the default scope is auto-detected from existing subdirectories so refresh respects the categories the user originally chose at `init --content`. AGENTS.md is excluded by default (almost always heavily customized; opt-in via `--include-agents-md`). Tool configs (`settings/`, `mcp/`, `hooks/`, `tools/`) are intentionally excluded — they have their own `customize` / `simplify` / `resolve` flow. Flags: `--only <csv>` to scope to specific categories (or opt into a category not yet in the tree), `--dry-run` to preview the plan, `--yes` for non-interactive use, `--include-agents-md` to surface AGENTS.md, `--help`. Documented in the `agentsync` skill template under "Pulling new template content into an existing project". 18 bats tests cover the full matrix (clean tree, conflict, --dry-run, --only, --include-agents-md, scope auto-detection, custom-files preservation, idempotency, non-TTY error, unknown values, `--only=value` syntax, nested skill references).

## 0.14.0

### Added

- **Commenting guidance baked into init templates:** `lib/templates/rules/comments.md` (always-on rule) and `lib/templates/skills/comments/SKILL.md` (on-demand skill) now ship with `agentsync init`, so freshly bootstrapped projects inherit a single language-agnostic policy on when to comment and what to leave out. The rule frames the default as "code over commentary" — make the code self-explanatory first, then comment only the _why_ a reader can't see (hidden constraints, external quirks, workarounds, surprises). The skill layers in two contrasting code blocks (narration / AI-thought-trail anti-pattern → contract-doc-plus-quirk pattern) and an `Edge cases` list covering apologies-in-code, task/PR/caller references, untracked TODOs, line-by-line code translation, decorative banners, and commented-out blocks. Both files are written in imperative-positive form consistent with 0.13.0's template rewrite, so Claude Opus 4.6+/4.7 follows them more reliably than the equivalent `Don't` lists. Addresses the recurring failure mode where Opus leaves `// Step 1:`, `// loop through users`, `// AI thought:` narration scattered through generated code.

## 0.13.0

### Changed

- **Templates and prompts rewritten in positive form:** every rule template, skill template, and the `agentsync generate` prompt (`lib/templates/AGENTS.md`, `lib/templates/rules/*`, `lib/templates/skills/agentsync/SKILL.md`, `lib/templates/skills/prompt-engineering/{SKILL.md,references/*}`, `lib/prompts/generate.md`) now phrase constraints as required behaviors instead of `Don't` lists, `## Anti-Patterns`, or `## What Not To Do` sections. Aligned with Anthropic's prompt-engineering guidance — Claude Opus 4.6+/4.7 follow positive instructions ("respond in flowing prose") more reliably than prohibitions ("don't use bullet points"), and over-comply with aggressive `MUST/NEVER/CRITICAL` framing. Section headers map predictably (`## Anti-Patterns` → `## Discipline`, `## What Not to Commit` → `## Keep Out of Commits`, `## What Never Goes In` → `## Keep Out of History`), and the bullet rewrites preserve the same constraints in imperative-positive form. `agentsync generate` and the `agentsync` skill itself now teach this pattern, so freshly bootstrapped projects inherit it.

### Added

- **`prompt-engineering` skill — Opus 4.7 specifics:** the Claude entry under `Tool-specific notes` expanded from a single line into a structured sub-section covering effort levels (`low / medium / high / xhigh / max`) with defaults for coding and agentic work, the `thinking: {type: "adaptive"}` syntax that replaces deprecated `budget_tokens`, subagent-spawning behavior changes on 4.7, tool-use undertriggering, the more-direct tone shift, the persistent cream/serif/terracotta frontend default and how to override it, and the deprecation of prefilled assistant messages on 4.6+. Lets maintainers tune prompts that were authored against earlier Opus versions without re-reading the full Anthropic guide.
- **`prompt-engineering` skill — three new snippets** in `references/snippets.md`:
  - `Frontend variety — propose options before building`: replaces the lost `temperature` knob for design variation on Opus 4.7 (which has a persistent default house style) by having the model propose 4 distinct directions before committing.
  - `Subagent control`: dual-direction guidance for steering subagent fan-out — reining in 4.6's over-spawning on trivial tasks, and prompting 4.7 (which spawns fewer by default) to delegate when it should.
  - `Multi-context-window workflow`: extends the existing state-tracking snippet with concrete patterns from Anthropic's agentic guidance — `init.sh` setup script, structured `tests.json`, freeform `progress.txt`, git checkpointing, and the fresh-context recovery sequence (`pwd` → progress notes → tests → git log → integration test) before resuming work.

## 0.12.3

### Fixed

- **`agentsync add` headings:** scaffolded rule/skill/subagent files now derive the top-level `# Heading` from the kebab-case name (`code-reviewer` → `# Code Reviewer`) instead of emitting the raw lowercase identifier (`# code-reviewer`). Matches the heading convention used by every built-in skill (`# Commit`, `# Code Review`) so freshly added files don't need a manual rename pass.

## 0.12.2

### Fixed

- **`agentsync update` silently exited with code 1:** if the `VERSION` file had no trailing newline, `read -r VERSION < VERSION` returned 1 (EOF without `\n`), and `set -euo pipefail` killed the script before a single character reached the terminal — `agentsync update` looked like it was hanging or doing nothing. `bin/agentsync.sh`, `update`, and `release` now tolerate a newline-less `VERSION` and fall back to the previous value if the read fails. The repo's `VERSION` file is also rewritten with a trailing `\n` so existing installs heal on the next pull.

## 0.12.1

### Fixed

- **`agentsync add` skill/subagent frontmatter:** scaffolded `SKILL.md` and `agents/<name>.md` files now emit YAML-safe frontmatter — `name` values are quoted and `description` uses a folded block scalar (`>-`). Previous templates broke YAML parsers (and downstream tooling that lints frontmatter) whenever the description contained a colon, quote, parenthesis, or multilingual example. The `add` substitution maps the new sentinel names (`"content"`, `"template-agent"`) back to the user-provided name on top of the existing `{{NAME}}` rewrite, so the templates themselves stay parseable on disk before substitution.
- **`agentsync generate` prompt:** the AI prompt that bootstraps `.ai/src/` now instructs the model to emit the same YAML-safe frontmatter (`name: "..."`, `description: >-`) for skills, commands, and subagents, plus a final reminder to mentally validate every generated file before output. Stops the generated bundle from landing with frontmatter that fails to parse the moment a description contains punctuation.

## 0.12.0

### Added

- **Drift detection** (`.ai/.sync-manifest`): every successful `sync` now writes a content-addressed manifest — one SHA-256 hash per generated file, sorted, no timestamps. On the next run, AgentSync compares each destination against the manifest before writing. If a dest was edited manually since the last sync (typical IDE-iteration flow: tweak `.claude/rules/foo.md` directly while testing), `sync` aborts with the list of edited files instead of silently overwriting them. Commit `.ai/.sync-manifest` to git so CI catches a forgotten commit. Pass `--force` to discard the edits and rewrite from source. `doctor` adds a Drift section that lists each tracked file as ✓ in-sync, ⚠ edited, or ⚠ missing. `check` keeps its existing semantics (regenerate fresh in a temp tree, diff against the repo) — drift is caught by the diff step there.
- **`agentsync adopt <dest-file>`**: reverse of sync — promote a manual edit in a generated file back into `.ai/src/` as the new canonical content, then refresh the manifest entry so the next `sync` is drift-free. Resolves the destination through the same YAML targets `sync` uses (so `.claude/rules/core.md` → `.ai/src/rules/core.md`, `.claude/settings.json` → `.ai/src/tools/claude/settings.json`). Settings/MCP/hooks scaffold the canonical per-tool override path when none exists yet, matching the 0.11 layout. Supports `--dry-run` (unified diff + plan, no writes), `--yes` (skip TTY confirm), and refuses transformed targets up front (`cursor` rules with header injection, `merge_to_file` / `inline_into_agents` variants, `format=toml` / `format=amazonq_json` outputs) — the round-trip would corrupt the shared source.

### Changed

- **`sync --force`**: new flag bypasses the drift check and rewrites every dest from source. Existing behavior of `sync` (write everything) becomes the explicit `--force` path; the default is now drift-aware.
- **`check` runs sync with `--force` internally**: the in-temp regenerate-and-diff loop is unchanged for users; the flag prevents drift detection inside the temp tree from short-circuiting the diff that produces the actual "out of sync" report.

## 0.11.2

### Fixed

- **`local x="$(...)"` masking subshell exit codes in `customize` / `list`:** the `_show_payload` source-label resolution and the `cmd_list` shared-MCP hint both initialized `local` variables from a command substitution on the same line, which silently swallows the subshell's exit status (ShellCheck SC2155). Split into separate declaration + assignment so a failing color/format helper now surfaces under `set -e` instead of leaving the caller with a half-built label.
- **Unused `label` read in `doctor` secret scan:** `_doctor_scan_file` parsed a `label` field from each secret pattern but never used it, so a malformed pattern row was harder to diagnose. Dropped the dead read; behaviour unchanged.

## 0.11.1

### Changed

- **`init` `Next steps` now points at `agentsync generate`:** the post-init summary previously listed only `list` / `enable` / `sync`, leaving newcomers to discover the AI-assisted bootstrap flow on their own. The new step ("Run `agentsync generate` — print an AI prompt to tailor `.ai/src/` to your codebase") sits right after the `AGENTS.md` edit hint, so a fresh project can go from `init` → AI-generated rules/skills without reading the README first.
- **`generate` clipboard tip is now platform-aware:** the trailing "Tip: run `agentsync generate | pbcopy`" hint hardcoded macOS's `pbcopy`, which prints noise on Linux where the binary doesn't exist. `generate` now probes `pbcopy` → `wl-copy` → `xclip -selection clipboard` → `xsel --clipboard --input` and prints the tip only when one of them is available, so Linux users see the right command and headless environments see no tip at all.

## 0.11.0

### Added

- **Per-tool override layout** (`.ai/src/tools/<tool>/<resource>.<ext>`): all tool-specific payloads (settings, hooks, per-tool MCP) live alongside their tool YAML under a single directory, so customizing a tool no longer scatters files across `.ai/src/{hooks,mcp,settings}/`. The resolver reads the new path first and falls back to the legacy flat layout for backward compatibility.
- **Shared MCP source** (`.ai/src/mcp.json`): one file describes the MCP servers every enabled tool should receive. Sync propagates it as `.mcp.json` / `.cursor/mcp.json` / etc., matching each tool's destination. Per-tool overrides (`.ai/src/tools/<tool>/mcp.json`) still win for the rare cases that need a different map.
- **`agentsync add mcp <server>`**: append (or create) an MCP server entry in the shared `.ai/src/mcp.json` without hand-editing JSON. Supports `--url`, `--command`, `--args`, `--env`, `--force`, and validates the merge with `python3`.
- **`agentsync migrate`**: moves legacy flat-layout overrides to the canonical per-tool layout (`mv` per file, dry-run by default, `--apply` to persist). When every legacy `.ai/src/mcp/*.json` is byte-identical it offers to consolidate them into the shared `.ai/src/mcp.json` (accept in TTY with `Y`, in non-TTY with `--yes`).
- **`enable <tool>` scaffolds editable copies**: after adding a tool to `tools.enabled`, `enable` copies the base settings / hooks templates into `.ai/src/tools/<tool>/` so the user has a concrete file to edit. MCP is deliberately not scaffolded — it resolves through the shared file. Pass `--no-scaffold` to opt out, `--scaffold` to force. TTY users get a single `Scaffold editable copies for <tool>?` confirm; non-TTY defaults to scaffold.
- **`enable` edit-paths block**: after enabling, each tool now prints a short block pointing at exactly which file to edit (`.ai/src/tools/<tool>/settings.json`), where shared MCP lives, and which commands unlock the rest (`agentsync customize <tool> hooks`, `agentsync add mcp`).
- **`doctor` Edit paths section**: lists each enabled tool with `✓` for existing overrides and `·` for payloads still inheriting from base, with the exact `customize` / `add mcp` command to materialize them.
- **`update` migration banner**: after pulling a 0.11+ release into a project still on the flat layout, `update` prints a banner pointing at `agentsync migrate --apply` so nothing silently breaks on a future release.

### Internal

- **Shared edit-paths formatter** (`lib/helpers/edit_paths.sh`): `enable` and `doctor` both reach through a single `tool_edit_paths_rows` helper for "what can the user edit for this tool?" instead of each maintaining its own resolution logic. Two thin formatters (`_block` for enable, `_checklist` for doctor) hang off it. Side effect: the `enable` block no longer prints a phantom `.ai/src/mcp.json` path when the shared file doesn't exist yet — it points at `agentsync add mcp <server>` directly.

### Changed

- **`init` `Next steps` now advertises the customization surface** (`agentsync add mcp <server>` and `agentsync customize <tool> <resource>`) so new users discover shared MCP and per-tool overrides without digging into the README.
- **`doctor` legacy-layout warning** now points at `agentsync migrate --apply` instead of suggesting re-running `customize` per file.
- **`resolve_payload_source` resolution order** (canonical as of 0.11): `.ai/src/tools/<tool>/<resource>.<ext>` → explicit `targets.<resource>.source` in the tool YAML → `.ai/src/<resource>/<tool>.<ext>` (legacy, emits one-shot deprecation warning) → `.ai/src/mcp.json` (for MCP only) → shipped base template.

### Migration

- Legacy flat-layout overrides (`.ai/src/{hooks,mcp,settings}/<tool>.<ext>`) are still read; sync continues to work unchanged. A one-line warning prints per run when a legacy file is the effective source. `agentsync migrate --apply` moves every legacy file into the canonical per-tool layout and, when safe, consolidates identical MCP copies into `.ai/src/mcp.json`. Legacy paths will be removed in 0.12.
- Projects wanting to opt out of the new `enable` scaffolding (e.g. CI provisioning scripts) should append `--no-scaffold` to their `agentsync enable` invocations.

## 0.10.2

### Fixed

- `agentsync init` crashed on macOS (bash 3.2) with `${default,,}: bad substitution`. The new interactive-prompt helper used the bash 4+ lowercase expansion `${var,,}`, which doesn't exist in macOS's stock `/bin/bash`. Replaced with a portable `tr '[:upper:]' '[:lower:]'` pipeline so `init` (and any future `prompt_confirm` caller) works on macOS's shipped bash without requiring `brew install bash`.

## 0.10.1

### Fixed

- `agentsync simplify` aborted under `set -euo pipefail` when no payload overrides were present because the payload pass returned a non-zero "nothing matched" code and `set -u` tripped on an unbound `any_considered` on exit paths. Switched to an explicit `_SIMPLIFY_PAYLOAD_MATCHED` out-flag and removed the unbound reads so `simplify` with no overrides now prints the friendly "nothing to simplify" message as intended.

## 0.10.0

### Added

- **Minimal `init`:** fresh projects get `.ai/agent_sync.yaml` + `AGENTS.md` + starter rules — payload files for hooks / MCP / settings are no longer eagerly copied for every supported tool. Opt in per tool with `--tools <csv>` or let auto-detection (`.claude/`, `.cursor/`, `CLAUDE.md`, ...) union in what you already use. A Claude-only project drops from ~20 scaffolded files to 3.
- **Interactive `init` wizard:** running `agentsync init` in a TTY opens a multiselect for tools (base catalog + auto-detected preselected) and content sections, then shows a plan + confirm before writing. Non-TTY (CI, scripts) skips the wizard silently. `--yes` accepts defaults in a TTY, `--dry-run` prints the plan without writing.
- **Base + override for hooks / MCP / settings:** `sync` now resolves each payload the same way tool YAMLs already do — project override (`.ai/src/<resource>/<tool>.<ext>`) wins, otherwise the shipped base template at `lib/templates/<resource>/<tool>.<ext>` is used. Delete an override and the base flows through on the next sync; no more per-tool payloads gating on files that have to exist somewhere.
- **`agentsync customize <tool> <resource>`:** dedicated path to create a payload override. `customize cursor hooks` copies the base hooks template into `.ai/src/hooks/cursor.json`; same pattern for `mcp` and `settings`. Hooks get a security gate — the base content is displayed first, and in non-TTY mode `--yes` is required before scaffolding.
- **`agentsync show <tool> <resource>` / `diff <tool> <resource>`:** inspect any payload resource — `show` tags `base` vs `★ user override`, `diff` prints a unified diff against the base.
- **Secret scanning in `doctor`:** `.ai/src/{mcp,settings,hooks}/*` are regex-scanned for common credential shapes (OpenAI/Anthropic `sk-*`, GitHub `ghp_*` / `github_pat_*`, AWS `AKIA*`, Slack `xox[baprs]-*`, Google `AIza*`, JWT). Placeholders (`${VAR}`, `<PLACEHOLDER>`) are ignored. JSON files are also syntax-validated when `python3` or `node` is available.
- **Version pinning:** `init` writes `agentsync_version: "<VERSION>"` at the top of `agent_sync.yaml`. `doctor` warns when the pinned version differs from the current CLI and suggests `agentsync upgrade-config` — the new command to re-pin.
- **`agentsync simplify` extended to payloads:** in addition to trimming redundant fields from tool YAML overrides, `simplify` now detects byte-identical payload overrides (scaffolded copies the user never edited) and offers to delete them so future updates to the base flow through automatically. `--apply -y` deletes non-interactively, `--apply` prompts in a TTY.
- **`list` resources column:** each tool row now shows `H M S` — hooks / MCP / settings indicators. Lowercase letter = base template available, `*` = user override present, `·` = no base for that resource. Summary line counts payload overrides separately from tool overrides.

### Changed

- **`init` no longer eagerly copies payloads for every tool.** Projects that opt in explicitly (`--tools claude,cursor`) or via auto-detection still get scaffolded copies; everything else falls through to base templates at sync time. Existing projects with per-tool payloads in `.ai/src/{hooks,mcp,settings}/` continue to work unchanged.
- **`customize <tool>` signature is now `customize <tool> [<resource>]`.** Without a resource argument it behaves exactly as before (tool YAML override). `<resource>` accepts `tool` (default), `hooks`, `mcp`, or `settings`.

### Migration

- Existing projects are not required to change anything. Run `agentsync simplify --apply` to clear out scaffolded-but-unedited payload overrides — the base templates will take over on the next sync without a behavior change. Run `agentsync upgrade-config` to re-pin `agentsync_version`.

## 0.9.0

### Added

- **`agentsync simplify [<tool>] [--apply] [-y]`:** walks every user override in `.ai/src/tools/` and drops fields whose value already matches the current base template. Trims the side effect of `customize --full` over time — redundant fields silently pin stale values and block upstream improvements, and `simplify` clears them out in one pass.
- **Dry-run by default:** prints a grouped preview — `Redundant (match base)`, `Kept (diverge from base)`, `Kept (no base value)` — so nothing is written until you pass `--apply`. When all fields match base, the preview reports the override file would be deleted outright.
- **`--apply -y` deletes emptied override files:** after `--apply` removes every redundant field, if the file has no real `key: value` content left, it's deleted automatically with `-y`. Without `-y`, an interactive shell prompts `[y/N]`; in non-TTY contexts (CI) the empty file is kept untouched.
- **Idempotent:** re-running `simplify --apply` on an already-minimized override is a no-op. Safe to add to pre-commit or CI hygiene checks.

## 0.8.0

### Added

- **Upstream drift detection on `agentsync update`:** before pulling a new release, the CLI snapshots the install-dir tool catalog and compares it against the new base catalog field-by-field. When an upstream change lands on a field you have overridden in `.ai/src/tools/<tool>.yaml`, the update prints a grouped warning — `<tool>: <field> — base changed from X to Y, your override is Z` — so silent upstream improvements can't be masked by a stale override.
- **`.ai/.pending-resolutions.yaml` queue:** conflicts surfaced by an update are persisted to this file (schema 1, with `from_version`, `to_version`, and the full before/after/override triple per conflict). Acts as an actionable to-do list between an update and your next resolve pass.
- **`agentsync resolve` reads the pending queue:** flags each conflicted field with `⚡` (vs. the default `◆`) and prints a banner listing how many fields were queued by the last update. Walking every override clears the queue automatically.
- **`agentsync update --strict`:** non-zero exit when any upstream change collides with a user override. Intended for CI — blocks a merge until someone reviews the drift.

## 0.7.0

### Added

- **`agentsync add <kind> <name>`:** scaffolds new source content with the right frontmatter and placement — `rule` → `.ai/src/rules/<name>.md`, `skill` → `.ai/src/skills/<name>/SKILL.md`, `command` → `.ai/src/commands/<name>.md`, `subagent` → `.ai/src/agents/<name>.md`. Refuses existing files by default; pass `--force` / `-f` to overwrite. Names are validated against path separators, `..`, leading `.` or `-`, and non-`[A-Za-z0-9_-]` characters — no surprise writes outside the `.ai/src/` tree.
- **Content templates:** new `lib/templates/content/{rule,skill,command,subagent}.md` ship minimal stubs with the `{{NAME}}` placeholder and the conventions each kind expects (`USE WHEN` clauses for skills, `## Gotchas`, `$ARGUMENTS` / `` !`cmd` `` hints for commands, `model` + `tools` frontmatter for subagents).

## 0.6.0

### Added

- **Layered tool configs:** tool YAMLs now follow an ESLint `extends` / Kustomize-style model. Each tool has a hidden base template in the install-dir catalog; users create per-field overrides in `.ai/src/tools/<tool>.yaml` that merge on top of the base. Fresh updates to base fields flow automatically to any field a user hasn't customized — no silent loss of upstream improvements.
- **`agentsync enable` / `disable`:** explicit opt-in/out of tools via the project `agent_sync.yaml` `tools.enabled` list. Replaces per-tool `enabled: true` flags (legacy form still recognized with a deprecation warning from `doctor`).
- **`agentsync customize <tool>`:** creates an empty override stub in `.ai/src/tools/<tool>.yaml` (or `--full` to copy the entire base for heavy editing). Stub points users to `agentsync show <tool> --base` for reference.
- **`agentsync show <tool>`:** prints the effective merged config for a tool, marking each field as `base` or `user`. `--base` prints just the upstream template.
- **`agentsync diff`:** lists tools with user overrides and shows which fields diverge from base.
- **`agentsync resolve`:** interactive walkthrough of diverging fields — `[k]eep` the override, `[a]dopt` the base value (removes the field so inheritance resumes), or `[s]kip`. Read-only notice in non-TTY contexts.
- **`agentsync doctor`:** four-section health check (project layout, enabled tools, user overrides, source directories). Exit codes 0/1/2 for clean / warnings / fatal. Flags legacy `enabled: true` overrides and missing base templates.
- **Auto-detection on `init`:** detects existing tool markers (`.claude/`, `.cursor/`, `.github/copilot-instructions.md`, etc.) and pre-fills `tools.enabled` so users don't have to manually opt in tools they're already using.
- **`list` markers:** ● enabled, ○ available, ★ customized — at-a-glance view of which tools are active and which have user overrides.

### Changed

- **`agentsync init` no longer scaffolds `.ai/src/tools/`** — the base catalog is hidden in the install-dir. Tools are opted in via `agentsync enable <tool>`. Existing projects with per-file `.ai/src/tools/*.yaml` overrides continue to work unchanged.
- **Sync engine reads layered configs:** `sync.sh` and `check.sh` now resolve each field via the layered lookup (user override → base template → built-in default) instead of reading a single YAML per tool. Sync output is unchanged for users who don't customize.
- **Empty `tools.enabled`** in `agent_sync.yaml` is now emitted inline as `enabled: []` (was multi-line with a stray empty-list marker).

### Fixed

- **`set -u` safety:** empty-array expansions in `enable.sh` no longer trip `unbound variable` under strict mode when no unknown tools are passed.
- **`disable` no-op bug:** `PROJECT_CONFIG_PATH` resolution was missing from the enable/disable context, causing `is_tool_enabled` to always return false. Now resolved and exported consistently across `enable`, `disable`, `customize`, `show`, `diff`, `doctor`, `resolve`.
- **YAML list append with inline `[]`:** appending a dash-item to `enabled: []` previously produced invalid YAML. The inline form is now rewritten to a bare `key:` before the item is appended.

## 0.5.4

### Changed

- **Update check:** runs in the background on every command instead of blocking once every 24 hours. The notification appears on the next invocation after a newer version is detected — zero latency on any command.

## 0.5.3

### Fixed

- **Sync performance for disabled tools:** dest paths are no longer parsed or resolved for tools with `enabled: false` when cleanup is off. Previously, each skipped tool triggered ~9 `parse_yaml_value` calls and up to 8 path-resolution calls before the enabled check — causing noticeable lag with several disabled tools.

## 0.5.2

### Fixed

- **Update check for help commands:** `help`, `--help`, and `-h` now trigger the update check alongside other interactive commands, so users see version notices when asking for help.

### Changed

- **`.gitignore`:** moved `.mcp.json` and `CLAUDE.md` exclusions outside of the `.claude/` directory scope to match their new root-level destinations (introduced in 0.5.1).

## 0.5.1

### Changed

- **Claude `CLAUDE.md` now writes to project root** instead of `.claude/CLAUDE.md`. Both paths are valid per Claude Code docs, but root is the canonical team-shared location shown in the best-practices guide and aligns with the AGENTS.md cross-tool spec used by Cursor / Codex / Windsurf.
- **Claude `.mcp.json` now writes to project root** instead of `.claude/.mcp.json`. Claude Code only auto-discovers project-scope MCP servers from `./.mcp.json` — the previous `.claude/.mcp.json` location was never picked up as project scope.

### Fixed

- **`tests/sync_options.bats`** — corrected `.cursor/AGENTS.md` assertions to root `AGENTS.md` (broken since 0.5.0 moved Cursor's agents dest to root).

## 0.5.0

### Added

- **Per-tool settings, MCP, and hooks coverage:** added missing canonical config targets across the matrix:
  - **Aider** — `settings → .aider.conf.yml` with auto-loaded `read: CONVENTIONS.md` (so the conventions file is actually picked up without `--read`).
  - **Amazon Q** — `mcp → .amazonq/mcp.json`; `subagents → .amazonq/cli-agents/*.json` via new MD→Amazon Q JSON converter (preserves `name`/`description`/`model`/`tools`).
  - **Cline** — `commands → .clinerules/workflows/*.md` (Cline slash commands).
  - **Codex** — `settings → .codex/config.toml` with `[mcp_servers.X]` template skeleton.
  - **Continue** — migrated from legacy `.continuerules` to canonical `.continue/rules/*.md` directory; `settings → .continue/config.yaml`.
  - **Cursor** — `commands → .cursor/commands/*.md` (Cursor 1.6 slash commands).
  - **Gemini CLI** — `settings → .gemini/settings.json` (combined config + MCP servers + hooks).
  - **Junie** — `skills → .junie/skills/`, `commands → .junie/commands/`, `subagents → .junie/agents/`, `mcp → .junie/mcp/mcp.json`.
  - **Windsurf** — `commands → .windsurf/workflows/*.md` (Cascade workflow slash commands), `hooks → .windsurf/hooks.json` (Cascade Hooks).
  - **Antigravity** — `commands → .agent/workflows/`.
  - **Zed** — `settings → .zed/settings.json` (holds `context_servers` for MCP).
- **MD→Amazon Q JSON converter** (`lib/helpers/format_conversion.sh`) — generic frontmatter parser now extracts `model:` and `tools:` (both inline `[a, b]` and YAML list forms); new `_json_escape` helper; new `convert_md_agent_to_amazonq_json` / `sync_agents_as_amazonq_json` functions; sync dispatcher accepts `format: amazonq_json`.
- **Frontmatter merge for rule sync** — new `merge_or_prepend_header()` in `lib/helpers/rule_operations.sh`. When a rule file already has frontmatter, the tool's default header only fills missing keys; source keys win. Lets users override `globs` / `trigger` / `applyTo` per rule for Cursor / Copilot / Windsurf without losing the tool default.
- **Templates:** new skeletons for `lib/templates/settings/{aider.yaml,codex.toml,continue.yaml,gemini.json,zed.json}`, `lib/templates/mcp/{amazonq.json,junie.json,windsurf.json}`, `lib/templates/hooks/{copilot.json,windsurf.json}`.
- **`enable_tools` test helper** in `tests/test_helper.bash` for opt-in test scenarios after the disabled-by-default change.

### Changed

- **AGENTS.md / GEMINI.md identity files now live at the canonical project root** for tools that follow the open spec — Cursor, Windsurf, Gemini CLI, Antigravity all moved their identity dest from `.<tool>/AGENTS.md` (or `.<tool>/GEMINI.md`) to the repository root. Aligns with the AGENTS.md cross-tool spec, deduplicates identical content across `.tool/` namespaces, and lets tools share one source of truth alongside Codex / Amp / Devin.
- **Junie** — agent identity moved from legacy `.junie/guidelines.md` to preferred `.junie/AGENTS.md` (per JetBrains docs); rules now inlined into the AGENTS.md (legacy `.junie/rules/` was unsupported).
- **Antigravity** — agent identity moved from `.agent/AGENTS.md` to canonical root `GEMINI.md`.
- **Claude `settings.json` template expanded** — added `model: sonnet`, `includeCoAuthoredBy: true`, `permissions.defaultMode`, an `ask` permission list, and an extended `deny` list covering secrets, PEM keys, and SSH keys.
- **Claude `rules` target** — removed redundant `append_imports: true`; Claude Code auto-discovers `.claude/rules/*.md` per docs, so the explicit `@import` block was double-loading content into CLAUDE.md.
- **Tool YAMLs** — stripped noise comments across all tool configs (`# X Configuration`, `# Reads AGENTS.md from root`, etc.) — the YAML structure is self-documenting.

### Removed

- **Amp, Devin, Tabnine** tool configs removed (not maintained against current docs; users with those tools can copy `_TEMPLATE.yaml`).
- Legacy `append_imports` usage in Claude scaffold and tests.

### Fixed

- **Tests aligned with disabled-by-default policy** — `tests/sync.bats`, `sync_options.bats`, and `check.bats` now call `enable_tools` after `init` instead of relying on shipped enabled defaults; `init.bats` asserts all tools default to `enabled: false`.

### Documentation

- **`agentsync` skill** rewrites — added Claude `settings.json` reference (model / env / statusLine / outputStyle / hooks-in-settings), Claude `.mcp.json` reference (stdio / http / `${VAR}` expansion), per-tool hooks comparison table (Claude / Cursor / Copilot / Codex / Windsurf), per-tool MCP comparison table, per-rule frontmatter override section.

## 0.4.2

### Changed

- **All tools disabled by default:** `agentsync init` now creates all 17 tool configs with `enabled: false`. Users explicitly enable only the tools they need via `.ai/src/tools/<name>.yaml → enabled: true`. Previously 6 tools (Claude Code, Cursor, Copilot, Gemini, Codex, Windsurf) were enabled out of the box.
- **`defaults.enabled` set to `false`:** the global fallback in `config.yaml`, `sync.sh`, and the scaffolded `.ai/agent_sync.yaml` now defaults to `false`, so tools that omit the `enabled` key are skipped rather than synced.

## 0.4.1

### Changed

- **`agent_sync.yaml` moved inside `.ai/`:** project config is now created at `.ai/agent_sync.yaml` instead of the repository root, keeping all AgentSync files in one place and simplifying export/import.
- **Backward compatible:** `sync`, `export`, and `import` check `.ai/agent_sync.yaml` first, then fall back to the legacy root-level `agent_sync.yaml` — existing projects continue to work without changes.
- **`init` respects both locations:** skips config creation if either path already exists.

### Fixed

- **`set -e` safety in `_resolve_source_paths`:** `[[ -n "" ]] && ...` without `|| true` caused silent exit under `set -e` when `agent_sync.yaml` keys were missing.

## 0.4.0

### Added

- **`agentsync export`** — bundles `.ai/src/` (rules, skills, commands, agents, settings, mcp, hooks, tools) and `agent_sync.yaml` into a single shareable `agentsync-bundle.tar.gz` archive.
- **`agentsync import <source>`** — imports agent config from three source types:
  - **GitHub URL** — downloads repository archive by branch (`--branch`, auto-detects `/tree/<branch>` in URL, falls back from `main` to `master`)
  - **Archive file** — extracts a `.tar.gz` / `.tgz` bundle (e.g. from `agentsync export`)
  - **Local directory** — copies `.ai/` and `agent_sync.yaml` from another project
- **Selective import** — `--only rules,skills` imports only specified targets
- **Diff preview** — both commands show a summary of new / updated / unchanged files before writing
- **Dry-run** — `--dry-run` on both export and import previews changes without writing
- **Confirmation prompt** — import asks before overwriting existing files (skip with `--force`)
- **Dynamic source paths** — export and import resolve source paths from `agent_sync.yaml` overrides and auto-detect `.ai/src/` vs `.ai/` (legacy) layout

## 0.3.0

### Improvements

- **`agent_sync.yaml` — full project config support:** `agentsync sync` now reads `defaults.enabled`, `defaults.cleanup`, `post_sync.allow`, and `post_sync.skip` from the project config file. Previously only `source.*` paths were honoured; all other settings were env-var-only or dead fields.
- **`defaults.enabled`:** tools that omit the `enabled` key in their YAML now fall back to `defaults.enabled` (default `true`) instead of hard-erroring.
- **`defaults.cleanup`:** setting `defaults.cleanup: false` prevents agentsync from deleting generated files when a tool is disabled.
- **`post_sync.allow` / `post_sync.skip`:** post-sync hook execution can now be controlled from `agent_sync.yaml`; `AGENTSYNC_ALLOW_POST_SYNC` and `AGENTSYNC_SKIP_POST_SYNC` env vars still take precedence.
- **`source.commands` and `source.subagents` overrides:** these two source paths can now be overridden in `agent_sync.yaml` just like `agents`, `rules`, `skills`, and `tools`.
- **`gitignore.update`:** setting `gitignore.update: false` in `agent_sync.yaml` disables automatic `.gitignore` management for projects that handle it manually or via another tool.

## 0.2.8

### Improvements

- **`agentsync init` generates `agent_sync.yaml`:** a project-level config file is now scaffolded in the repository root on first init, giving users a ready-made place to override source paths (`agents`, `rules`, `skills`, `tools`) without touching the global config.

## 0.2.7

### Improved

- **Line count guidelines added to `generate.md`:** each generated file type now has an explicit recommended size — `AGENTS.md` (40–70 lines), rules (20–50), skills (50–100), commands (15–40), agents (30–70) — so AI-generated configs stay focused and scannable.
- **Split-over-grow principle documented:** both `generate.md` and the `agentsync` skill now explicitly state that multiple small focused files are preferred over one large catch-all, for both rules and skills.
- **`agentsync` skill updated:** size guidelines and the split principle added to the "Writing Rules" and "Writing Skills" sections; AGENTS.md limit updated from `Under 60 lines` to `40–70 lines`.

## 0.2.6

### Fixed

- **ShellCheck SC2155:** split `export REPO_ROOT_CANONICAL="$(…)"` into two lines in `sync.sh` — assign first, then export — so a non-zero exit code from the subshell is not masked.

## 0.2.5

### Fixed

- **ShellCheck SC2034:** `REPO_ROOT_CANONICAL` in `sync.sh` marked as `export` so ShellCheck recognises it is consumed by sourced helper scripts (`helpers/paths.sh`) and stops reporting it as unused.

### Changed

- **Annotated git tags:** `agentsync release` now creates an annotated tag (`git tag -a`) whose message is the corresponding CHANGELOG.md section instead of a bare lightweight tag.
- **Auto-tag CI:** `auto-tag.yaml` likewise creates an annotated tag with the changelog body, so the tag object on GitHub carries the release notes.
- **GitHub Release body from CHANGELOG:** `release.yaml` now populates the GitHub Release description from the CHANGELOG.md section for the tagged version instead of a raw git-log dump.

## 0.2.4

### Fixed

- **Multi-version update changelog:** `agentsync update` now shows release notes for every version skipped during an update, not just the final one. Versions are displayed in ascending order (oldest → newest). Previously, jumping from e.g. v0.2.0 to v0.2.3 silently omitted the intermediate release notes.

## 0.2.3

### Code Quality

- **Sync engine modularized:** `lib/helpers/files.sh` (628 lines) split into four focused modules — `filters.sh`, `file_ops.sh`, `rule_operations.sh`, and `format_conversion.sh` — each with a single responsibility.
- **Path resolution extracted:** eight path utility functions moved from `sync.sh` into a dedicated `helpers/paths.sh`, reducing `sync.sh` by ~180 lines.
- **`cmd_init` refactored:** monolithic 227-line function broken into four private sub-functions (`_init_create_directories`, `_init_copy_source_templates`, `_init_copy_tool_configs`, `_init_print_summary`) with a thin orchestrator.

## 0.2.2

### Fixed

- **Shell arithmetic across all sync functions:** replaced `((count++))` with `count=$((count + 1))` in `sync_dir`, `copy_rules`, and `sync_rules` — prevents false exit code 1 when counter is zero under `set -e`, which caused `agentsync sync` to silently abort mid-run.

### CI

- **Windows support:** tests now run on `ubuntu-latest`, `macos-latest`, and `windows-latest`; bats installed via `git clone` on Windows, all steps use `shell: bash`.
- **Node.js 24:** added `FORCE_JAVASCRIPT_ACTIONS_TO_NODE24: true` at workflow level to silence Node 20 deprecation warnings.
- **Auto-tagging:** pushing to `main` with a changed `VERSION` file now automatically creates and pushes the corresponding git tag, triggering a GitHub Release.

## 0.2.1

### Fixed

- **`cd` safety:** `cd` calls in `update` and `release` commands now abort on failure (`|| exit 1`) instead of silently continuing in the wrong directory.
- **`rm -rf` guard:** destination path in `sync_dir` uses `${dest:?}` to prevent accidental root deletion if the variable is unset.

### CI

- ShellCheck: added `SC2039` and `SC2166` to the ignore list to suppress false positives on intentional bash-isms.

## 0.2.0

### Added

- `agentsync generate` — interactive mode with project description input
- `agentsync release [major|minor|patch]` — bump version, tag, and push
- Bats test suite — 101 tests covering all commands (cli, init, sync, check, generate, list, hooks, release)
- CI/CD via GitHub Actions — ShellCheck linting + tests on Ubuntu and macOS
- Automated GitHub Releases on tag push
- Update notification banner when a new version is available
- Symlink auto-repair on `agentsync update` (handles renames across versions)
- `init` now shows enabled tools list and improved next steps
- Migration guide in README for existing configurations

### Changed

- Default enabled tools reduced to top 6: Claude Code, Cursor, GitHub Copilot, Windsurf, Gemini CLI, OpenAI Codex
- Flattened `lib/system/` → `lib/`, `lib/system/lib/` → `lib/helpers/`
- Removed duplicate docs: `lib/README.md`, `lib/docs/STRATEGY.md`, `lib/system/README.md`

### Fixed

- Cross-platform `readlink` compatibility in update command
- macOS `sed` compatibility in generate command
- Disabled tool cleanup no longer deletes files owned by other enabled tools

## 0.1.2

- Initial public release
- 17 AI tools supported
- Sync engine with format conversions (MD → MDC, TOML, instructions.md)
- Git hooks for auto-sync
- `agentsync check` for CI validation
