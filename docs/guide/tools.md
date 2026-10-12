# Supported tools

Exuno ships a config for 15 coding tools. Turn them on with `exuno enable <tool>` and check them with `exuno list`. The ID in the second column is what `enable`, `--only`, and `--skip` take.

## What each tool receives

| Tool | ID | What sync writes |
| --- | --- | --- |
| Claude Code | `claude` | `CLAUDE.md`, rules, skills, commands, agents, `settings.json`, `.mcp.json`, write-guard hook |
| GitHub Copilot | `copilot` | `copilot-instructions.md`, `.instructions.md` rules, skills, `.prompt.md` commands, `.agent.md` agents, `hooks.json` |
| Cursor | `cursor` | `AGENTS.md`, `.mdc` rules, skills, commands, agents, `mcp.json`, `hooks.json` |
| Gemini CLI | `gemini` | `GEMINI.md` with rule references, skills, commands (converted to TOML), agents, `settings.json` |
| OpenAI Codex | `codex` | `AGENTS.md` with rule references, skills, commands (as `command-*` skills), subagents (converted to TOML), `hooks.json`, `config.toml` with settings and MCP |
| Kimi Code | `kimi` | `.kimi-code/AGENTS.md` with rule references, skills, commands (as `command-*` skills), `mcp.json` |
| MiniMax Code | `minimax` | `AGENTS.md` with rule references, project `.mcp.json` |
| OpenCode | `opencode` | `AGENTS.md` with rule references, skills, commands, subagents (safe Markdown), settings and converted MCP in `opencode.json`, `agentsync.ts` plugin |
| Devin Desktop (Windsurf) | `windsurf` | `AGENTS.md`, `.devin/` rules with trigger frontmatter, skills, workflows (commands), `mcp_config.json`, `hooks.json`; removes its old `.windsurf/` outputs |
| JetBrains Junie | `junie` | `.junie/AGENTS.md` with rule references, skills, commands, agents, `mcp.json` |
| Cline | `cline` | `AGENTS.md`, `.cline/` rules, skills, workflows (commands); removes its old `.clinerules/` outputs |
| Kiro | `kiro` | `AGENTS.md`, `.kiro/steering/` with inclusion frontmatter, skills, commands (as `command-*` skills), agents (converted to Kiro Markdown), `settings/mcp.json` |
| Amazon Q | `amazonq` | `00-context.md`, `.amazonq/rules/`, a skills index and a commands index, `mcp.json`, CLI agents (converted to JSON) |
| Zed | `zed` | `.rules` (`AGENTS.md` followed by all rules), a skills index and a commands index, `settings.json` |
| Google Antigravity | `antigravity` | `GEMINI.md`, `.agents/rules` with trigger frontmatter, `.agents/skills`, `.agents/workflows` (commands) |

MiniMax Code reads only files in the main project workspace, so it has no config-home [profile](workspaces-and-profiles.md#profiles-separate-config-homes): `profile add` and `sync` refuse MiniMax profile variants. MiniMax's project skill path isn't verified yet, so Exuno doesn't sync skills to it.

## Each kind of source, by tool

| Source | What it is | Tools |
| --- | --- | --- |
| `AGENTS.md` | Role, approach, principles. Copied as is under each tool's name. | All |
| `rules/` | One file per topic. Always on by default; `paths:` frontmatter makes a rule load only for matching files, using each tool's own trigger. Keep the always-on set short so each rule gets attention. | All |
| `skills/` | Recipes in the [Agent Skills](https://agentskills.io) format. See [Skills](skills.md). | All except MiniMax Code |
| `commands/` | Slash commands. `review.md` becomes `/project:review`. `$ARGUMENTS` and `` !`shell` `` work. | Native: Claude, Cursor, Copilot (`.prompt.md`), Gemini (TOML), Junie, Cline, Windsurf, Antigravity, OpenCode. As skills: Codex, Kimi Code, Kiro. As an index: Amazon Q, Zed |
| `agents/` | Subagents with their own context and tool limits. Frontmatter: `model`, `tools`, `readonly`. | Claude, Cursor, Copilot (`.agent.md`), Gemini, Junie, Codex (TOML), Amazon Q (JSON), OpenCode (safe Markdown), Kiro (Kiro Markdown) |
| `mcp.json` | One shared `mcpServers` map | Claude, Cursor, Windsurf, Junie, Amazon Q, Kimi Code, MiniMax Code, Kiro, OpenCode (converted), Codex (converted) |
| `tools/<tool>/settings.*` | Permissions and other settings. Claude's hooks go here too. | Claude, Gemini, Codex, OpenCode, Zed |
| `tools/<tool>/hooks.*` | Event hooks and project plugins, as JSON or TypeScript | Cursor, Copilot, Codex, Windsurf, OpenCode |

## Format conversions

| From | To | Used by |
| --- | --- | --- |
| Rule `.md` | `.mdc` with YAML frontmatter | Cursor |
| Rule `.md` | `.instructions.md` with an `applyTo` header | Copilot |
| Rule `.md` | `.md` with a `trigger: always_on` header | Windsurf |
| Rules | One merged file | Zed |
| Rules | A reference per rule (name and title) in the agents file | Codex, Gemini, Junie, Kimi Code, MiniMax Code, OpenCode |
| Skill folders | An index (name and description) in the agents file | Amazon Q, Zed |
| `AGENTS.md` | `00-context.md` in the rules folder | Amazon Q |
| `AGENTS.md` | Placed before the merged rules | Zed |
| Command `.md` | `.toml` (`prompt` field, `!{}` syntax, `{{args}}`) | Gemini CLI |
| Command `.md` | `.prompt.md` | Copilot |
| Command `.md` | A generated `command-<name>` skill | Codex, Kimi Code, Kiro |
| Agent `.md` | `.agent.md` | Copilot |
| Agent `.md` | `.toml` (`developer_instructions` field) | Codex |
| Agent `.md` | `.json` (Amazon Q CLI custom agent) | Amazon Q |
| Agent `.md` | Safe OpenCode Markdown (`mode` and permissions) | OpenCode |
| Agent `.md` | Kiro Markdown, with tools mapped to Kiro's names | Kiro |
| MCP `mcpServers` | OpenCode's top-level `mcp` map (`local` or `remote`) | OpenCode |
| MCP `mcpServers` | `[mcp_servers]` tables in `config.toml` | Codex |

## Models and providers are not tools

Exuno targets coding tools and the files they read, not model vendors. If you use Kimi or GLM models through Claude Code, Cline, or OpenCode, keep syncing that tool's target. There is no `glm.yaml` or `zai.yaml` on purpose. MiniMax Code is different because it's a standalone CLI with its own project files; a MiniMax model used through another tool still uses that tool's target.

- **A Kimi model in another tool.** Set up the provider with [Kimi's guide for third-party agents](https://www.kimi.com/code/docs/en/third-party-tools/other-coding-agents) and keep syncing your Claude, Cline, or OpenCode target. Use the `kimi` target only for the standalone Kimi Code CLI.
- **GLM Coding Plan.** Sign in to Z.AI with its official [Claude Code](https://docs.z.ai/devpack/tool/claude) or [OpenCode](https://docs.z.ai/devpack/tool/opencode) setup. Keep API keys in the provider's credential store or the environment, never in `.ai/src/`.
- **OpenCode MCP.** The shared `.ai/src/mcp.json` is converted into OpenCode's top-level `mcp` map. Use `.ai/src/tools/opencode/mcp.json` for a different server map. Before you turn on shared MCP, move any `mcp` field out of `.ai/src/tools/opencode/settings.json`; `sync` and `doctor` name both files when it's unclear which one owns MCP.
- **OpenCode hooks.** Customize `.ai/src/tools/opencode/hooks.ts`. Exuno owns only `.opencode/plugins/agentsync.ts` and leaves other plugins alone. Custom tools under `.opencode/tools/`, other plugins, themes, TUI preferences, and credentials stay OpenCode's.
- **Kimi hooks and agents.** Kimi Code has built-in agents but no way to add project agents. Its hooks live in the global `$KIMI_CODE_HOME/config.toml`, which Exuno leaves alone.

```bash
exuno enable kimi opencode     # the standalone coding tools
exuno sync

npx @z_ai/coding-helper        # set up GLM in a supported tool
opencode auth login            # choose Z.AI Coding Plan for OpenCode
```

## A tool that isn't listed

See [Adding a new tool](configuration.md#adding-a-new-tool).
