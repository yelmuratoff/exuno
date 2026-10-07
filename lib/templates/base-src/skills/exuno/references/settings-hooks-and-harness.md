# Settings, hooks, and harness changes

Read this when overriding a tool's settings or permissions, writing a hook, or turning an agent mistake into a harness change.

## Settings & Permissions

Each tool ships with base settings. To diverge, scaffold an override with `agentsync enable <tool>` or `agentsync customize <tool> settings` — it writes `.ai/src/tools/<tool>/settings.<ext>`, which wins over the base on sync. Delete the file to resume inheriting the base.

Permission decisions are three-valued — `allow`, `deny`, and `ask`. Keep `ask` populated: with only the two extremes, every uncertain case has to be filed as one of them, so either the agent proceeds unattended on something consequential or the entry is widened until it stops protecting anything.

**Tier by consequence, not by tool name.** Reads, listings, and pure analysis sit low; workspace mutation sits higher; anything irreversible or touching a shared system — push, hard reset, release, production credentials — sits higher again. `Bash(git status)` and `Bash(git push *)` are the same tool and belong in different tiers.

Example `.ai/src/tools/claude/settings.json`:

```json
{
  "permissions": {
    "allow": ["Bash(git status)", "Bash(git diff *)", "Read"],
    "ask": ["Bash(git push *)", "Bash(git reset --hard *)"],
    "deny": ["Bash(rm -rf *)", "Read(.env)", "Read(**/secrets/**)"]
  }
}
```

A permission entry is matched per tool call and cannot reason about intent across a sequence. When a boundary has to hold however the agent phrases the call, back it with a `PreToolUse` hook (below) instead of extending the pattern list.

## Hooks — advisory rules vs. enforced gates

Tool hook overrides live at `.ai/src/tools/<tool>/hooks.<ext>` and are scaffolded with `agentsync customize <tool> hooks`. OpenCode hooks render to AgentSync's owned project plugin; Kimi hooks are global-only and stay outside project sync.

The distinction that decides whether to write a rule or a hook: a rule in `AGENTS.md`/`rules/` is **advisory** — loaded as context the model can choose to ignore under pressure; a hook is **enforced** — the harness runs it every time, regardless of what the model decided. Reach for a hook when something _must_ happen, not merely _should_ — a commit-message check, say, rather than a rule asking the agent to remember it.

Claude Code's hook events (mechanics are per-tool — each receives the tool-call payload as JSON on stdin):

- **`SessionStart`** — runs once when a session begins, off the agent's turn. Use for one-time setup that shouldn't cost the agent context or steps: building or refreshing a code index, warming a cache, fetching the active ticket. A ~15–20s index build belongs here, not in a rule asking the agent to index itself.
- **`PreToolUse`** — runs before a tool call; **exit code 2 blocks the call** and sends the script's stderr back to the agent as feedback. Use to forbid an action (e.g. block a write to a protected path).
- **`PostToolUse`** — runs after a tool call; pipe the edited file through the formatter/analyzer/test command and feed failures back. This closes the verify loop _deterministically_ instead of trusting the agent to self-check.
- **`Stop`** — blocks the turn from ending until a check passes (Claude Code ends the turn anyway after 8 consecutive blocks, so the check must be able to converge).

## Guarding the generated files

Three layers keep an agent (and a person) editing the source instead of the output:

1. The shipped `AGENTS.md` and `rules/core.md` say where instructions live, so it is in context every session.
2. Claude Code gets a generated `PreToolUse` hook at `.claude/hooks/agentsync-guard.sh`, wired up by `hooks.PreToolUse` in the base settings. It checks the target path against `.ai/.sync-manifest` and exits 2 — blocking the write — with the source path to edit instead. Override it per project at `.ai/src/tools/claude/guard.sh`, or drop the `hooks` block from your settings override to remove it. The target declares `profile_scoped: false`, so config-home profiles share this one script rather than each getting a copy nothing invokes; `agentsync doctor` warns when a settings override never references it.
3. `agentsync sync` refuses to overwrite a generated file that changed since the last sync, and `agentsync adopt <file>` promotes such an edit back into `.ai/src/`.

## Turning an agent mistake into a harness change

Treat a mistake as a signal about a missing component rather than a war story to retell in chat. Per incident, or as a weekly habit:

1. **Describe the slip in two or three sentences** — what the agent did, what it should have done, what let it through.
2. **Name the missing component** — a rule line, a hook (the rule existed and was rationalized past), a narrower tool or permission, a skill that did not fire or whose description missed the phrasing ([`evaluating-skills.md`](evaluating-skills.md)), a reviewer check, a required test, or a context reset.
3. **Build the smallest one** and route it through the artifact-type sections of `SKILL.md`.
4. **Stack uncorrelated layers for a repeat offender.** An agent kept adding `retry:` to a `flutter test` case that was a race, not a flake: one rule line ("fix the cause of a failing test rather than adding a retry"), one PostToolUse hook that greps edited `*_test.dart` files for `retry:`, one line in the reviewer prompt. No layer had to be perfect; together they made the failure structurally hard to repeat.

Keep the rulebook alive with a periodic retrospective over recent session transcripts ([`evaluating-skills.md`](evaluating-skills.md) treats them as ground truth): a rule followed without prompting is a removal candidate, a rule broken is a sharpening candidate, an uncovered pattern is an addition. A model can draft the edits; a human merges them.
