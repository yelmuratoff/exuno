# {{TITLE}}

- One imperative constraint per bullet ("Use X", "Never Y") — not explanations.
- Keep this file focused on a single topic. Split if it grows beyond ~50 lines.
- Cross-cutting rules are always-on context for every task, so every line should
  change behavior. For a domain rule (state, routing, data…), add `paths:`
  frontmatter (a list of globs) so it loads only when matching files are touched
  instead of always — exuno translates it to each tool's native glob trigger.
