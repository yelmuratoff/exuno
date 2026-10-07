# Writing Skills

Full reference for authoring Exuno skills. Read this when creating or editing a skill in `.ai/src/skills/<name>/`.

Exuno skills follow the open [agentskills.io](https://agentskills.io) format — a portable standard supported by Claude Code, Codex, Cursor, Copilot, Gemini CLI, OpenCode, and ~30 other agents. Validate any skill with `skills-ref validate <path>`.

## Contents

- How skills load — progressive disclosure
- Skill directory layout
- Frontmatter fields
- Writing the description
- Structure of a good skill
- Calibration principles
- Choosing invocation and routing many skills
- Failure modes and diagnostic vocabulary
- Content hygiene
- Patterns for effective instructions
- Size budget
- Rule of three
- Iteration

## How skills load: progressive disclosure

Agents load skills in three stages, so design with each stage in mind:

1. **Discovery (~100 tokens, all skills):** the agent reads only `name` + `description` of every available skill at startup, deciding which might be relevant.
2. **Activation (≤5000 tokens, one skill):** when a task matches a description, the agent reads the full `SKILL.md` body.
3. **Resources (on demand):** files in `references/`, `scripts/`, and `assets/` load only when `SKILL.md` instructs the agent to read them.

**Implication:** keep `SKILL.md` lean. Move detail behind explicit triggers like _"Read `references/X.md` when the input is a multi-page PDF."_ Order the body highest-value-first — context compaction truncates from the end, so a buried instruction is the first thing dropped under pressure.

## Skill directory layout

```
my-skill/
├── SKILL.md          # Required: frontmatter + instructions
├── references/       # Optional: docs read on demand (REFERENCE.md, etc.)
├── scripts/          # Optional: executable code the agent runs
├── assets/           # Optional: templates, schemas, images
└── ...
```

Use **relative paths** from the skill root (`references/foo.md`, `scripts/bar.py`) and keep references **one level deep**.

## Frontmatter fields

```yaml
---
name: skill-name # Required. Must match the parent directory name.
description: One imperative sentence on what the skill does + concrete trigger conditions.
# Optional:
license: MIT
compatibility: Requires Python 3.12+ and uv
metadata:
  author: your-team
  version: "1.0"
allowed-tools: Bash(git:*) Read Grep # Experimental; tool-specific.
---
```

**`name` constraints (hard):**

- 1–64 characters
- Lowercase letters, digits, and hyphens only — no `_`, no uppercase, no Unicode
- No leading or trailing hyphen, no consecutive `--`
- Must equal the parent directory name (`my-skill/SKILL.md` ↔ `name: my-skill`)
- Unique across categories: `flutter/auth/` and `backend/auth/` would both land at `<dest>/auth/`, so sync refuses the pair — name one `flutter-auth`

**`description` constraints (hard):**

- 1–1024 characters
- Must convey _both_ what the skill does _and_ when to use it
- Must survive YAML parsing as a plain unquoted scalar: no `: ` (colon followed by a space) anywhere in the value, and no YAML-special first character (`[`, `{`, `>`, `|`, `*`, `&`, `!`, `%`, `#`, `@`, `` ` ``, `"`, `'`). One stray `: ` (e.g. `Gate: present a design`) makes the parser fail with "mapping values are not allowed" and the loader **silently skips the entire skill** — it never triggers and nothing flags the loss. Rephrase with `—` instead of `: `; quoting or `>-` folded scalars also work but a plain dash keeps the single-line style consistent.

After any frontmatter edit, prove it parses before moving on — `skills-ref validate <path>`, then `exuno skills check` for the metadata Exuno reads.

## Writing the description (the trigger)

The description is the only thing the agent sees during discovery. Vague = invisible.

Derive the trigger from concrete cases, not from guessing: before writing the description, list the real tasks and the phrasings a user would actually bring ("rotate this PDF", "why does it crash on launch", "why is it failing") — the keywords and contexts below fall out of that list instead of being invented.

- **Domain keywords first, then the trigger.** The first ~50 characters carry the match on their own once a host truncates the listing, so open with the domain nouns the user would say and put "Use when…" after them. "Use this skill when…" as the opener spends those characters on nothing.
- **Focus on user intent, not internal mechanics.** Describe what the user is trying to achieve, not the steps the skill takes.
- **Be pushy about phrasings, narrow about the domain.** Explicitly list contexts where the skill applies, _including ones where the user doesn't name the domain_ ("even when phrased as 'this is broken' or 'why is it failing'") — but keep the trigger to the workflow the skill actually serves. "Create and validate Postgres schema migrations. Use when adding or changing a migration, or reviewing its rollout." beats "…Use when working with databases, queries, models, or persistence.", which fires on every database task and loads guidance that doesn't help it (OpenAI's GPT-6 Astra guidance; Codex also shortens every description once many skills are installed, so a long one is read in part).
- **Pack relevant keywords** the user might say or type, including alternate phrasings.
- **As short as the trigger allows.** A domain phrase plus one "Use when…" clause usually carries it; descriptions here run about 200 characters. 1024 is the hard limit, not a target.

Bad: `description: Helps with testing.`
Good: `description: Tests for a feature, bug, or regression — unit, integration, end-to-end, coverage. Use when adding tests or asking why one fails, even when phrased as "make sure this works".`

For the trigger-tuning workflow — should-trigger / should-not-trigger eval queries, the train/held-out split, and why some queries can't be fixed by any description — read [`evaluating-skills.md`](evaluating-skills.md). Deeper external write-up: [agentskills.io/skill-creation/optimizing-descriptions](https://agentskills.io/skill-creation/optimizing-descriptions).

## Structure of a good skill

```markdown
---
name: example-skill
description: <imperative + trigger conditions, 1–1024 chars>
---

# Skill Name

One line: what this skill does and when to invoke it.

## Bundled references (load on demand) # Optional, only if you have references/

- `references/X.md` — read when [concrete trigger condition]
- `references/Y.md` — read when [concrete trigger condition]

## Steps # The core procedure

1. Concrete numbered steps with real commands and paths.
2. Use imperative verbs.

## Output format / template # Optional, when format matters

\`\`\`
<concrete template the agent fills in>
\`\`\`

## Gotchas # The highest-signal section

- Every mistake the agent has made using this skill.
- Concrete corrections to wrong assumptions ("the `users` table uses soft deletes; queries must include `WHERE deleted_at IS NULL`").
- Edge cases and common pitfalls.
```

The skeleton above is the default; the body's _shape_ must match the work it documents:

- **Workflow** — one sequential procedure → numbered steps, branch with a decision tree where order forks.
- **Task collection** — several independent operations under one domain → a section per task.
- **Reference / guidelines** — standards to apply rather than steps to run → a flat set of rules, each with a worked example.
- **Capabilities** — an integrated system → a capability list that points at the script or reference behind each.

Most skills lean on one shape and borrow from another; pick the dominant one and let the section structure follow it rather than forcing every skill into the same outline.

## Calibration principles

These come straight from the [agentskills.io best-practices guide](https://agentskills.io/skill-creation/best-practices). Internalise them.

- **Add what the agent lacks; omit what it knows.** Don't explain what a PDF is, what HTTP does, or how `git` works. Jump straight to project-specific conventions, non-obvious edge cases, and the particular tools or APIs to use.
- **Procedures over declarations.** Teach _how to approach_ a class of problems, not the answer to one specific instance. The procedure must generalise even when individual details are concrete.
- **Defaults, not menus.** Pick one tool/library/approach and mention alternatives briefly. "Use `pdfplumber`; fall back to `pdf2image` for scanned PDFs" beats listing four equal options.
- **Match specificity to fragility.** Be prescriptive on fragile, sequence-sensitive operations ("run exactly: `python migrate.py --verify --backup`"). Be descriptive on flexible work ("look for SQL injection, weak auth, race conditions") and let the agent's judgment fill in. A skill written as an itinerary for an older model now overconstrains a current one — OpenAI reports that guidance which helped GPT-5.6 Sol hinders GPT-6 Astra, and Anthropic says the same of skills written for prior Claude models — and a repo skill also steers other contributors' agents on other models, so write for the least prescriptive reader the repo will see.
- **Aim for moderate detail.** Concise stepwise guidance with a working example beats exhaustive documentation. When you're tempted to cover every edge case, ask whether the agent can handle most by judgment.
- **Design coherent units.** A skill encapsulates one workflow that composes well with others. Too narrow → many skills load for one task. Too broad → can't be activated precisely.

## Choosing invocation, and routing many skills

Pick model-invocation only when the agent must reach the skill on its own, or another skill must reach it — its description then costs a permanent slice of the discovery budget every turn. Pick user-invocation (the consuming tool's manual-only flag, e.g. Claude Code's `disable-model-invocation: true`) for anything that only ever fires by hand; it costs no budget but spends the user's memory, since they become the index of what exists. When user-invoked skills multiply past what one person can recall, add a single user-invoked router skill that names the others and when to reach for each — it can only point at them, never fire them, because a user-invoked skill has nothing but the human to match its description against.

## Failure modes & diagnostic vocabulary

Name the symptom so you can reach for the cure. Common skill-quality failures:

- **Premature completion** — a step ends before the work is truly done because attention slipped to _being_ done. Cure — sharpen the completion criterion first; split the sequence only if it stays fuzzy.
- **Sprawl** — length itself, even when every line is live. Cure — push reference behind load-triggers (see _Size budget_).
- **Sediment** — stale lines that accumulate because adding feels safe and removing risky. Cure — prune on every edit.
- **Duplication** — one meaning in two places; it drifts apart on the next edit. Cure — one home per idea, cross-link the other.
- **No-op** — a line the model already obeys by default, so it spends tokens saying nothing. Cure — test it against the no-guidance default; if behavior is unchanged, cut it.

The positive levers the cures lean on — a **completion criterion** that is checkable and exhaustive, a **leading word** (a compact pretrained concept repeated as a token to anchor behavior cheaply), and **predictability** (the same process every run) as the root virtue the others serve.

Four more named levers sharpen where each piece of content sits:

- **Information hierarchy** — a skill's content ranked by how immediately the agent needs it: in-file steps (primary) → in-file reference → disclosed reference behind a pointer. Push down whatever you can so the top stays legible; in-file reference that should be disclosed buries the steps under it and turns attending to them into a coin-flip.
- **Context pointer** — a reference held in context that names out-of-context material and encodes when to reach it; the **description** is the top-level one (window → skill), `references/` triggers are the same object one level down. Its _wording_, not its target, decides when and how reliably the agent reaches the material — so a must-have target behind a weak pointer is a variance bug — sharpen the wording first, and inline the material only if that fails.
- **Co-location** — keep a concept's definition, rules, and caveats under one heading rather than scattered, so reading one part brings its neighbours with it. The within-file companion to the hierarchy — the hierarchy decides _how far down_ a piece sits, co-location decides _what sits beside it_ once there.
- **Legwork** — the behind-the-scenes digging within a single step (reading files, exploring the codebase) rather than offloading to the user. It is never its own step — it lives latent in the wording, raised by a leading word (_thorough_, _relentless_) or a _demanding_ completion criterion. This is why "every modified model accounted for" drives more work than "produce a change list", and the demand axis binds flat reference too ("every rule applied"), so a skill with no steps still carries an exhaustiveness bar.

Finally, name the two costs that **granularity** (how finely you split skills) spends, so a split has to earn one of them — a **model-invoked** description costs **context load** (tokens and attention in the window every turn); a **user-invoked** skill costs **cognitive load** (the human must remember it exists and when to reach for it, per _Choosing invocation_ above). Split by invocation only for a distinct leading word that should trigger on its own; split a run of steps by sequence only when the _post-completion steps_ still ahead tempt the agent into premature completion.

## Content hygiene

- **Don't date the content.** A skill carries no "as of" timestamp, so branches like "before August 2025, do X" rot silently. Write the current guidance as the only guidance; when a practice is genuinely superseded, collapse the old one into a short `## Old patterns` section (or `<details>`) at the end rather than leaving conditional branches in the body.
- **Pick one term per concept and repeat it.** Drifting between "endpoint", "route", "URL", and "path" for the same thing forces the agent to infer they're synonyms. Consistent vocabulary reads as a tighter spec.

### Sentence mechanics for instruction prose

Four rules borrowed from ASD-STE100, the controlled English that aircraft maintenance manuals are written in. Its constraint is the same one an instruction file has — a sentence that can be read two ways gets acted on the wrong way — so the mechanics transfer even though the domain doesn't.

- **Put the condition before the command.** "If the tests fail, revert" beats "Revert if the tests fail". An agent that acts on the first clause has already reverted by the time it reaches the qualifier. This is the single highest-value one, and it applies to every `unless`, `only when`, and `except` in the file.
- **One instruction per sentence.** Two actions share a sentence only when they genuinely happen together. A compound sentence lets the agent satisfy the first half and count the line as done.
- **Say must, can, or will.** A requirement is _must_, a possibility is _can_, a future fact is _will_. "Should", "may", and "might" hand the agent the decision about whether the rule applies to this case. Reserve them for the cases where the choice really is the agent's.
- **Keep the articles and the "that".** "Make sure that the file exists" over telegraphic "Ensure file exists". Function words are the cheapest disambiguation available; dropping them saves a few tokens and costs a parse.

## Patterns for effective instructions

Pick the ones that fit your task; not every skill needs all.

- **Gotchas section.** Concrete corrections, not generic advice. Update every time the agent makes a mistake using the skill — this is the single highest-leverage section to maintain.
- **Output templates.** When format matters, show a concrete template the agent fills in — pattern matching beats prose description. Inline for short templates; in `assets/` for long or conditional ones.
- **Checklists.** Multi-step workflows with dependencies benefit from an explicit progress list (`- [ ] Step 1: …`) so the agent tracks state and doesn't skip steps.
- **Validation loops.** "Do X → run validator → fix issues → repeat until validation passes." More reliable than asking the agent to "double-check".
- **Plan-validate-execute.** For batch or destructive operations: extract source-of-truth → produce a plan in a structured file → run a validator script that checks the plan against the source → only then execute. The validation script's error messages should give the agent enough to self-correct.
- **Bundled scripts.** If you notice the agent reinventing the same logic across runs (chart-builder, parser, validator), write the script once in `scripts/` and have `SKILL.md` invoke it. State explicitly whether the agent should run the script or read it as reference. Use forward-slash relative paths, declare runtime/package requirements, handle expected failures with actionable output, and test the script in every intended host/model environment. When two or more scripts share setup or state, promote them to one CLI and keep the skill as the cookbook that teaches its commands.
- **Cheatsheet / decision-rules layer.** For a reference skill, the highest-value layer is not term→definition rows (that's a glossary) or prose (that's the body) — it's the judgment that lets the agent _decide_ without re-reading. Every line resolves a choice: decision rules ("when X, do Y, because Z"), decision trees for branches >2, trade-off matrices scoring options on the dimensions that matter, thresholds & defaults (the specific numbers and rules of thumb), and tells & smells (fast heuristics for recognising a situation). Put it in `references/cheatsheet.md` and keep it to a printable page.

## Size budget

- Hard recommendation: **`SKILL.md` ≤ 500 lines and ≤ 5000 tokens.** This is the body the agent loads on activation and shares context with everything else.
- Soft target for sleek skills: 50–150 lines if the workflow is simple. Don't pad to fill space.
- **When you legitimately need more,** move detail to `references/<topic>.md` and reference it with a concrete load-trigger ("read `references/X.md` when Y"). Don't dump it inline.
- **Give a reference file over ~100 lines a table of contents at the top.** Agents often preview a long file with a partial read (e.g. `head -100`) before committing to the whole thing; without a ToC they misjudge its scope and skip the section they needed.
- **For a reference file over ~10k words, put grep/search patterns in the SKILL.md pointer** ("grep `references/api.md` for the endpoint name") so the agent jumps to the slice it needs instead of loading the whole file into context.

## Rule of three

Don't create a skill for everything. If you've done something three times manually and want it consistent next time, _then_ create a skill. Earlier than that, you don't yet know the shape.

When you do create it, draft from the real trace of those repetitions rather than from memory of the idealised workflow — the tools you actually reached for, the step sequence, and the corrections you made mid-way. When the prompt is "turn this into a skill", that trace is the conversation you just had — mine it for the steps and the dead-ends before writing. The friction points you hit are exactly the gotchas the next agent needs.

## Iteration

Skills improve through real execution, not introspection.

1. **Refine with traces.** Run the skill on real tasks. Read execution traces — not just final outputs. Wasted steps and unproductive branches usually mean an instruction is too vague, doesn't apply, or presents too many options without a default.
2. **Add gotchas as you go.** Every correction you make in a real session is a candidate gotcha. Save it before you forget.
3. **For high-stakes skills, run evals.** Define test cases (`evals/evals.json`), run with-skill vs. without-skill, grade outputs against discriminating assertions, and read the benchmark for what the averages hide. Read [`evaluating-skills.md`](evaluating-skills.md) for the full loop — baseline choice, discriminating assertions, blind comparison, and trigger optimization. External write-up: [agentskills.io/skill-creation/evaluating-skills](https://agentskills.io/skill-creation/evaluating-skills).
