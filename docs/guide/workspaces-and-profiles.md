# Workspaces and profiles

A workspace is a parent project with Exuno projects inside it, which share some content. A profile is a second config home for the same tool, for example a work Claude next to a personal one.

## Workspaces: nested projects

A parent project at `workspace/.ai/src/` can have child projects below it (`workspace/foo/.ai/src/`, `workspace/bar/.ai/src/`). There are two ways to handle content they share.

### Inherit with `shared:`

Each child lists the categories it takes from the parent:

```yaml
# child/.ai/exuno.yaml
shared:
  path: "../"
  inherit: rules,skills,commands,agents
```

During a sync, Exuno builds a temporary `.ai/src/` in memory: the child's files, with the parent's files filling the gaps. When both have the same file, the child's wins. Every enabled tool's output is written from that combined tree. This works the same for tools that don't read parent folders (Codex, Cursor, JetBrains Junie) and for those that do (Claude Code). Inherited files are never written into the child's `.ai/src/`.

### Clean up copies with `dedupe`

When a child has a copy of one of the parent's files, `dedupe` compares them by hash:

```bash
exuno dedupe                  # find the nearest parent .ai/src/ (stops at the git repository boundary)
exuno dedupe --against ../    # name the parent explicitly
exuno dedupe --workspace      # every nested .ai/, deepest first, then alphabetically
exuno dedupe --yes            # no prompts: delete identical copies, leave different ones
```

For identical files it asks `[d]elete / [k]eep / [v]iew`. Deleting a shipped template also adds a `template_overrides.declined` entry so `refresh` won't offer the file again. For files with the same path but different content, it shows a diff and leaves the decision to you; `dedupe` never resolves a difference on its own.

### What `doctor` reports

`exuno doctor` looks for the nearest parent `.ai/src/` the same way and reports identical copies as advisories and differing ones as info. Rules and skills with `category: governance` in their frontmatter are raised to advisories when they differ, with a note that the difference is probably a mistake rather than an override. These findings never change the exit code, so a CI job or pre-commit hook that runs `doctor` doesn't fail on them.

### Syncing the whole workspace

`exuno sync --workspace` runs `sync` in every Exuno project below the current folder, deepest paths first and siblings in `LC_ALL=C` order so the order is always the same. It carries on past a failing project and exits with the highest exit code it saw. The other sync options (`--only`, `--skip`, `--profile`, `--dry-run`, `--force`, `--quiet`, `--json`) are passed to each project, so `--json` prints one object per project.

Looking for a parent stops at the git repository boundary, so a child with its own `.git` never picks up an unrelated `.ai/src/` above it.

## Profiles: separate config homes

If you sync from `$HOME` and use two accounts with the same tool, such as a work Claude and a personal Claude, a profile generates a second, self-contained config home (`~/.claude-hub/` next to your personal `~/.claude/`). Its content is the base `.ai/src/` plus profile-only additions. Shared rules stay shared; work-only rules and MCP servers live in the profile.

```bash
exuno profile add hub                       # a "hub" profile for every enabled tool
exuno profile add hub --tools claude,codex  # only these tools
exuno profile add hub --adopt               # bring an existing ~/.claude-hub/ into the profile first
exuno profile list                          # profiles, their tools, and config homes
exuno profile remove hub                    # delete the profile and its config-home output
```

`profile add` writes three things:

- a small variant tool, `.ai/src/tools/<tool>-hub.yaml`, which inherits everything from the base tool through `base:` and changes only the destinations
- an overlay folder, `.ai/profiles/hub/src/`, for profile-only rules, skills, commands, agents, and `AGENTS.md` (the profile wins when names collide)
- a `profiles:` block in `exuno.yaml`

`exuno sync` builds every active profile along with your personal tools; `exuno sync --profile hub` builds just that one. The profile's overlay goes on top of the base and on top of a `shared:` overlay if you have one, so the two combine. Run the tool against the result with its config-home variable, for example `CLAUDE_CONFIG_DIR=~/.claude-hub claude`.

Profile outputs are kept out of git and protected against hand edits like any other generated file. Some tools can't have profiles: MiniMax Code reads only project files, and a tool YAML can say `profile_supported: false`.
