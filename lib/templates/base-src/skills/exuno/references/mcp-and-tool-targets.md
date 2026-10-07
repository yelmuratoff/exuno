# MCP, inline options, and adding a tool

How servers and generated content reach each target: registering MCP servers per host, the inline fallbacks for tools without their own rules, skills, or commands directory, and onboarding a new tool.

## MCP Configs

One shared `.ai/src/mcp.json` reaches every compatible enabled tool, so you define a server once. Add servers with `exuno add mcp <name>` or edit the file directly:

```json
{
  "mcpServers": {
    "playwright": {
      "command": "npx",
      "args": ["-y", "@anthropic/mcp-playwright"]
    }
  }
}
```

When one tool needs a different server map, `.ai/src/tools/<tool>/mcp.json` shadows the shared file for that tool only. `exuno customize <tool> mcp` scaffolds it when the target ships a copyable base. `exuno mcp use --merge --apply` extends only a regular per-tool `mcp.json`; use `--replace <id>` to replace the selected ID explicitly.

Where the shared file lands decides whether a server is live, so check the host before assuming:

- **Claude Code** reads `.mcp.json` at _project_ scope only — the file beside the project's `.git`. A `$HOME` sync therefore writes a `~/.mcp.json` Claude never opens; user-scope servers live in `<config-dir>/.claude.json`, which only the `claude mcp` CLI writes. For a global home, set `targets.mcp.enabled: false` in the Claude tool YAML and register servers through a `post_sync` hook that drives `claude mcp add` from the same source; `claude mcp list` confirms the result. `post_sync` runs only with the out-of-repo consent Exuno requires — export `EXUNO_ALLOW_POST_SYNC=true` from the shell rc.
- **Codex** composes supported server fields into `.codex/config.toml` while preserving the settings source; keep `[mcp_servers.*]` out of settings when a separate MCP source exists. Codex-native fields (`cwd`, `enabled`, `startup_timeout_sec`, and the rest listed in `docs/mcp-library.md`) belong in a per-tool `.ai/src/tools/codex/mcp.json`; to keep servers in the settings file instead, set `targets.mcp.enabled: false` in `.ai/src/tools/codex.yaml`.
- **OpenCode** is composed rather than copied: canonical `{ "mcpServers": { ... } }` input becomes the top-level `mcp` map in `opencode.json`. Put a divergent map at `.ai/src/tools/opencode/mcp.json`, and keep `mcp` out of the OpenCode settings override while canonical MCP exists; `sync` and `doctor` reject ambiguous ownership instead of overwriting either source. OpenCode reads its global config from `~/.config/opencode/opencode.json`, so a global home points both the `settings` and `mcp` dests there; the base `opencode.json` dest is right only for a project sync.
- **Kimi Code** — `targets.mcp.format: kimi_json` makes `exuno mcp use` write Kimi-native HTTP entries without `type: "http"`; sync then copies that per-tool source. For hand-written remote servers in a shared source, use a Kimi per-tool override with its native `url` shape. Kimi hooks are global-only in `$KIMI_CODE_HOME/config.toml` and stay outside project sync.

**Files a tool also writes.** Synced from `$HOME` or into a profile, `ownership: auto` (the default on `settings` and `mcp` targets) owns only the declared keys of each TOML or JSON file, one entry per MCP server, and keeps what the tool writes itself: Codex's projects, hooks.state, and plugins, Claude Code's model, theme, and plugin choices, a server added in an editor. Keep that state out of `.ai/src`, and use `exuno adopt <file>` to pull a declared key the tool changed back into its source. Zed's commented settings stay owned whole.

## Inline Options

For tools without separate rules/skills directories, use inline options:

- **`inline_into_agents: true`** (rules) — appends lightweight rule REFERENCES (name + title) to the agents file instead of syncing rules as separate files. Used by: Codex, Gemini, Junie, Kimi Code, OpenCode.
- **`inline_into_agents: true`** (skills) — appends lightweight skill INDEX (name + description) to the agents file instead of syncing skills as directories. Used by: Amazon Q, Zed.
- **`as_skills: true`** (commands) — emits each `.ai/src/commands/<name>.md` as a generated skill at `<targets.skills.dest>/command-<name>/SKILL.md`. For tools that have a skills dir but no native slash-command surface. Requires `targets.skills.dest`. Used by: Codex, Kimi Code, Kiro.
- **`inline_into_agents: true`** (commands) — appends a `## Commands` index (one `` `/<name>` — description `` line per command) to the agents file. For tools that have neither a commands dir nor a skills dir. Requires `targets.agents.dest` (or `rules.merge_to_file` fallback). Used by: Amazon Q, Zed.
- **`prepend_agents: true`** (rules with `merge_to_file`) — prepends AGENTS.md content before merged rules in a single output file. Used by: Zed.
- **`00-context.md` pattern** — for directory-based tools without separate agents support, AGENTS.md is copied as `00-context.md` inside the rules directory. Used by: Amazon Q.
- **`legacy_dest`** (any target) — the path an earlier release of the tool config wrote that target to. Sync removes only the files there that the previous manifest records, then the directories that leaves empty, so a tool reading both paths does not load them twice; hand-written files stay, and a failed sync restores them. Used by: Windsurf (`.windsurf/` → `.devin/`), Cline (`.clinerules/` → `.cline/`).

## Adding a New Tool

1. Start from the closest shipped tool: `exuno customize <tool> --full` writes `.ai/src/tools/<tool>.yaml`, which you copy to `.ai/src/tools/<new>.yaml`. Every field is documented in `lib/templates/tools/_TEMPLATE.yaml` in the Exuno repository; a project has no copy of it, and a `_`-prefixed file is never read as a tool.
2. Set `name`, `enabled: true`, and configure `targets`.
3. Run `exuno sync --only <tool>` to test.
