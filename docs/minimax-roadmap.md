# MiniMax Code target

[PR #7](https://github.com/yelmuratoff/agent_sync/pull/7) proposed a
MiniMax target for the retired Bash engine by mirroring OpenCode's project
files. It is not a branch to merge into the Rust engine: the review found
unresolved output differences and ambiguous auto-detection, and its version
bump belongs to a separate release.

The [MiniMax Code source README](https://github.com/MiniMax-AI/minimax-code/blob/main/README.md)
documents a current terminal CLI and `AGENTS.md` project guidance. The
[project MCP contract](https://github.com/MiniMax-AI/minimax-code/blob/main/packages/local-runtime-v2/docs/project-mcp.md)
also confirms that Desktop, TUI, `mcode exec`, and `mcode acp` read the primary
workspace's `.mcp.json` with a top-level `mcpServers` map. The shipped
`minimax.yaml` therefore covers these two confirmed project surfaces. It does
not claim that MiniMax consumes OpenCode's `opencode.json`, `.opencode/`, or
plugin files.

Future expansion needs first-party evidence or an installed-client check for
project-level skills, commands, subagents, settings, and hooks. Keep credentials
and user-global state out of Exuno. The `.mcp.json` destination is shared
with Claude Code; differing effective MCP sources must fail before either
client's configuration can overwrite the other. Release versioning follows
the normal release workflow.

Revisit additional surfaces when client evidence makes their generated files
testable. The current target intentionally remains smaller than PR #7.
