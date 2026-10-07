---
name: prompt-engineering
description: Design, debug, and refine prompts for LLM-based coding tools, agents, and pipelines. Use this skill when writing or rewriting a system prompt, agent persona, slash-command body, rule, or skill description; tuning a prompt that produces vague, incomplete, off-format, hallucinated, or over-/under-triggering output; choosing among techniques like XML tags, few-shot examples, role-setting, reasoning steps, or recap; debugging a model that ignores instructions, leaks information, or over-engineers solutions; or designing the persona, autonomy posture, or tool-use rules of an autonomous agent — even when the user does not explicitly mention "prompt engineering" or "system prompt". Not for Exuno file structure, frontmatter, or scaffolding new rule/skill/command files — use the `exuno` skill for that.
---

# Prompt Engineering

A workflow and toolbox for writing prompts that steer modern LLMs reliably — system prompts, slash commands, skills, rules, agent personas, and one-off queries.

## Bundled references (load on demand)

- **`references/snippets.md`** — read **when you need a copy-paste prompt fragment** for a recurring concern: anti-overengineering, parallel tool calls, default-to-action / do-not-act, investigate-before-answering, code-review coverage, frontend aesthetics, frontend variety / propose-options, subagent control, persistence across context windows, multi-context state tracking, reversibility/safety, verbosity, long-document grounding, keep-going stop rules, and the GPT-6 Astra steering set. ~25 vetted blocks.
- **`references/metaprompting.md`** — read **when a prompt under-performs and the cause is unclear**: full metaprompting template, filtering procedure for the model's suggestions, when to use vs. skip.
- **`references/agent-persona.md`** — read **when designing or auditing the system prompt of an autonomous agent**: identity, autonomy posture, tool-use rules, editing constraints, full Friendly and Pragmatic personality blocks, final-message format, plan discipline.

## Workflow

1. **Define success first** — write one sentence: *what must the output be, and for whom*. Define the eval criteria *before* the prompt. If you can't, the prompt isn't ready.
2. **Draft with the minimal components** — Objective + Instructions + (if needed) Context, Role, Format, Examples. Add a component only when you can justify it.
3. **Build a test set** — 3+ diverse inputs: happy path, edge cases, malformed input. Tiny is fine; some signal beats none.
4. **Run and grade** — code-grade where possible (JSON/regex/Python parse, length, keyword presence), model-grade for quality, human-grade for nuance. Always demand reasoning + score from a model grader, not a bare score.
5. **Diagnose before patching** — name the cause (ambiguous scope? missing context? conflicting instruction? wrong format?). Fix the cause, not the symptom. When the cause is unclear, run metaprompting (see `references/metaprompting.md`).
6. **Change one thing at a time** — otherwise you won't know what worked.
7. **Iterate** — re-run the same test set, compare versions; stop when stable across all of them, not when "looks good once".
8. **Pin and version** — for production, pin to a specific model snapshot (e.g., `claude-opus-5-5`, `gpt-6-astra`) and re-run evals when upgrading — a model upgrade shifts behaviour silently and your evals are the only signal.

## Prompt components (include only what's load-bearing)

- **Objective / goal** — what success looks like. Concrete and measurable ("summary ≤ 3 sentences", not "brief summary").
- **Instructions** — ordered, imperative steps when order matters.
- **Role / persona** — one sentence shifts tone and domain lens. "You are a senior Flutter engineer" beats "You are a helpful assistant". For full agent personas, see `references/agent-persona.md`.
- **Context** — background the model can't derive. Put *long* context near the top, query at the bottom (30%+ quality lift on multi-doc tasks).
- **Constraints** — what it must / must not do. State *scope explicitly* ("apply to every section, not just the first"). Two flavours of specificity:
  - *Type A — attributes:* qualities of the output (length, tone, structure). Useful in almost every prompt.
  - *Type B — steps:* the reasoning path the model should follow. Use when the natural approach would miss something.
- **Output format** — structure, length, schema. Show it in an example if non-trivial. Use a widely-parsable format (JSON / XML / Markdown / YAML) unless you have reason not to.
- **Few-shot examples** — 3–5, relevant, diverse, wrapped in `<example>` / `<examples>` tags. Place them *after* instructions, not before. Add a one-line rationale per example explaining *why* the output is ideal — this teaches the pattern, not just the surface.
- **Reasoning step** — "think step by step" or `<thinking>` tags when the task needs multi-step logic. On models with adaptive/native thinking (Claude 4.6+, GPT-5 and GPT-6 reasoning, Gemini Thinking), prefer goal-only guidance ("think thoroughly") over prescriptive steps — the model often outperforms a hand-written plan. On models where thinking can't be disabled (Claude Opus 5.5, Fable 5.1) delete "think carefully" and "think step by step" lines outright — they delay the reply without improving it — and use the effort setting instead; for a simple question say "Answer directly."
- **Recap** — add a closing reminder only when an eval shows a requirement is lost without it, and keep the requirement in one owning section: both Anthropic and OpenAI report that repeated and over-prescriptive instructions cost quality on current models.

## Techniques that reliably work

- **Golden rule**: show your prompt to a colleague with no context. If they'd be confused, the model will be too.
- **Spend the prompt on non-default behavior** — much of what older prompts spelled out is now default model behavior, and the marginal instruction can hurt: OpenAI reports that guidance which helped GPT-5.6 Sol over-constrains GPT-6 Astra, and Anthropic says the same of skills written for prior Claude models. Start from the smallest prompt that passes your evals and add a line only when an eval exposes a gap.
- **Explain the *why***: "never use ellipses" → "your response is read by TTS, which can't pronounce ellipses." The model generalises from the reason.
- **Tell it what to do, not what to avoid**: "respond in flowing prose" beats "don't use bullet points".
- **Prefer positive examples over negative ones.**
- **Use XML tags** (`<instructions>`, `<context>`, `<input>`, `<example>`) to separate sections — especially effective on Claude. Name them descriptively (`<sales_records>` beats `<data>`); the tag name itself signals what's inside.
- **Name custom tools semantically** — `semantic_search` over `search`, `apply_patch` over `edit`. Tool names and argument names should look "in-distribution" for what the model was trained on.
- **Match prompt style to desired output** — markdown-heavy prompts produce markdown-heavy responses; plain prose begets plain prose.
- **Be explicit about tool use** — "change this function" not "can you suggest some changes", unless suggestion is the goal.
- **Ground long-document tasks in quotes first** — ask the model to extract relevant quotes before answering.
- **Verification** — a self-check instruction is a hypothesis, not proof: verify code by execution and claims against a source. Current models verify unprompted (Claude Opus 5 re-checks its own work; GPT-6 Astra runs tests on its own), so a blanket "double-check before finishing" line adds re-verification rather than catching more — add it only where an eval shows a miss.
- **Metaprompting** — when a prompt under-performs without an obvious cause, ask the model to propose changes to its own instructions. Full procedure in `references/metaprompting.md`.
- **Start from a vetted snippet** — for any of the recurring concerns listed at the top, drop in the relevant block from `references/snippets.md` rather than writing from scratch.
- **Truncate long tool outputs deterministically** — for agentic loops, cap tool output at ~10k tokens; if exceeded, keep the first half and last half with `...N tokens truncated...` between. Random or only-tail truncation throws away signal.

## Common failure modes and fixes

| Symptom                                        | Likely cause                           | Fix                                                                                                            |
| ---------------------------------------------- | -------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| Output drifts off-format                       | Format not specified or only described | Show the format in an example; use XML/JSON schema.                                                            |
| Answers are vague or generic                   | Objective is vague                     | Replace qualitative words ("brief", "good") with measurable ones ("≤3 sentences").                             |
| Model only suggests, doesn't act               | Verb is passive                        | Use imperative ("change", "write"), not "can you suggest". See `default_to_action` in `references/snippets.md`.|
| Instruction applied to first item, not all     | Scope not stated                       | "Apply to every X, not just the first".                                                                        |
| Output too verbose                             | No length cap; markdown-heavy prompt   | Cap length; remove markdown from the prompt; ask for "concise, focused" responses.                             |
| Model over-triggers tools / skills             | Aggressive language ("MUST", "ALWAYS") | Soften to "use when..."; modern models over-comply with ALL-CAPS directives.                                   |
| Over-engineered / extra files                  | No scope constraint                    | Drop in the anti-overengineering snippet from `references/snippets.md`.                                        |
| Hallucinated code / file references            | No grounding requirement               | Drop in the `investigate_before_answering` snippet from `references/snippets.md`.                              |
| Shallow reasoning on hard tasks                | Effort too low / thinking off          | Raise `effort` (Claude) or `reasoning_effort` (OpenAI) before re-prompting.                                    |
| Extended thinking on trivial questions         | Prompt implies complexity, or a "think carefully" line on a model that can't disable thinking | Delete the think-carefully line and lower effort first; then add "Answer directly when the task doesn't need multi-step reasoning". |
| Turn ends on "Next I'll…" / asks permission for requested work | Model treats the run as pair programming; GPT-6 Astra stops after a first implementation | `keep_going` in `references/snippets.md` (Opus 5.5 names the stops it makes); on GPT-6 Astra define completion up front and add the bias-to-action block. |
| Tests or checks beyond the task's scope        | "Run the tests and check your work" lines on a model that already does | Remove the line; on GPT-6 Astra add the test-calibration block from `references/snippets.md`.                  |
| Cause unclear despite reading the prompt       | Prompt has accreted edits              | Run metaprompting (`references/metaprompting.md`).                                                             |

## Prompt health checklist

Before shipping a prompt, check:

- [ ] Typos, grammar, punctuation clean.
- [ ] No undefined jargon or acronyms.
- [ ] No ambiguous qualifiers ("good", "brief", "appropriate") — replaced with measurable ones.
- [ ] No conflicting instructions or examples.
- [ ] No redundant restatements of the same rule.
- [ ] Role and output format defined when the task needs them.
- [ ] Edge cases and missing-data handling addressed.
- [ ] Not trying to do too many distinct tasks in one pass — split if so.
- [ ] No emotional manipulation ("very bad things will happen") — doesn't help modern models; often hurts.
- [ ] Untrusted user input is isolated (XML tags, labelled boundaries) to resist injection.
- [ ] For production: pinned to a specific model snapshot.

## Tool-specific notes

A model fact belongs here only when it changes the words in a prompt or their placement; parameters, limits, prices, and error codes come from the vendor's live docs (Anthropic: the "Prompting Claude <model>" page for each model; OpenAI: `developers.openai.com/api/docs/guides/latest-model`). Re-run the effort sweep and the prompt's evals on every migration: level names don't mean the same depth across models, and families pull in opposite directions.

- **Claude (Opus 5.5, Fable 5.1, Sonnet 5)** — instructions are followed literally, so state scope explicitly ("apply to every section, not just the first"). Prefer XML tags. Dial back `MUST/ALWAYS/CRITICAL` — they over-trigger. Reasoning depth is the `effort` setting, not a sentence; on Opus 5.5 and Fable 5.1 thinking can't be disabled, so delete "think carefully" lines and lower effort instead. Prefill is gone since 4.6 — format through instructions, XML output tags, or structured outputs. Key levers:
  - **Long runs** — Opus 5.5 keeps going better than Opus 5 but sometimes ends the turn to report: a summary that announces the next step, an offer to continue, a list of choices that don't block the work. It follows instructions that name those stops, and the stops you do want (`keep_going` in `references/snippets.md`). Keep the confirmation before destructive actions.
  - **Progress notes** — Opus 5.5 writes short between-call updates, but the API returns them as thinking blocks that are empty at the default display; confirm the client shows them before adding cadence lines. Opus 5 narrates readily (tune down); Fable 5.1 goes quiet (tune up).
  - **Verification and delegation** — Opus 5 verifies and delegates without being told, so remove "double-check" and "use a subagent to verify" lines and cap subagents (`subagent_control`). Opus 5.5 sustains long audits and migrations across parallel subagents; ask it to check each subagent's evidence and re-test the cap rather than carrying it over.
  - **Refusals** — Opus 5.5 and Fable 5.x run biology, cybersecurity, and reasoning-extraction classifiers. Never ask the model to reproduce its internal reasoning in the reply (read summarized thinking instead); finding vulnerabilities in source code stays allowed.
  - **Frontend default** — without direction the model falls back on a house style, and "avoid a generic AI look" swaps one default for another. Name the specific patterns to leave out ("no cream background, italic accent words in headings, numbered 01/02/03 labels, monospace labels, pill buttons"), or have it propose options first (`frontend_propose_options`).
  - **Pasted text** — wrap text the user pasted from elsewhere in `<pasted_content id="…">` tags with a per-block random id and tell the model to follow instructions inside only where the user's own message asks (`pasted_content` in `references/snippets.md`).
  - **Vision** — Opus 5.5 reads charts, diagrams, and screenshots precisely without tools; re-test scaffolding built for earlier models. A crop or image-processing tool still adds accuracy on the densest inputs.
- **OpenAI GPT-6 (Astra / Sol / Luna)** — uses `instructions` + `input` roles; `instructions` applies to the current request only and is not carried by `previous_response_id`, so send it every turn. Keep prompts in code (reusable prompt objects are being retired). Reasoning depth is `reasoning.effort`; change it mid-conversation with a `configuration_update` item so the cached prefix survives. GPT-6 Astra runs opposite to the Claude 5 family: it asks the user more, delegates less, tests on its own, defaults to lists and tables, stops after a first implementation, and over-reads ask-first boundary language written for an older model — define completion before the run, grant the safe workflow explicitly, and use the _GPT-6 Astra steering set_ in `references/snippets.md`. It is also more sensitive to instructions in skills and `AGENTS.md`, so audit every instruction file it can reach.
- **Gemini** — responds well to the full component template (Objective → Instructions → Constraints → Context → Output format → Examples → Recap). Native Thinking: avoid explicit step-by-step. Read the current model guide at `ai.google.dev/gemini-api/docs/latest-model` before carrying any Gemini-specific line forward.
- **Agent rules / skills / commands (this repo)** — rules are always-on constraints (imperative, 20–50 lines, one topic). Skills are on-demand recipes with a triggering description. Commands are one-workflow prompts. See the `exuno` skill for Exuno file format and scaffolding.

## Gotchas

- Re-test prompts when crossing model families — autonomy posture runs in opposite directions (Claude Opus 5.5 keeps going, GPT-6 Astra stops early and asks), so a prompt tuned on one misfires on the other.
- When the model misbehaves, audit existing rules for ambiguity or conflict before adding a new one. Metaprompting (`references/metaprompting.md`) often surfaces these.
- Separate instructions from raw data using XML tags or clear headings — a mixed paragraph blurs scope.
- Place the question at the bottom of a long prompt — bottom placement performs meaningfully better than top.
- Show the schema or use the tool's native structured-outputs feature when you need JSON. Prose alone drifts off-format.
- On Claude 4.6+, reach for structured outputs or explicit format instructions. Prefilled assistant messages are deprecated.
- Run an eval for every non-trivial prompt change — tuning on vibes drifts you off-target.
- Define what "good output" means before optimising. The eval criteria come first; the prompt serves them.
- Pin production prompts to a specific model snapshot. Model upgrades silently shift behaviour, and your evals are the only signal.
- Ask a model grader for *strengths, weaknesses, reasoning, and score* together. A bare score collapses to default-middling 5–7s.
- Check effort and reasoning settings before blaming the prompt — shallow output at low effort is a settings issue, not a prompt one.
- Pick one personality block per agent persona. Stacking Friendly and Pragmatic cancels them out (see `references/agent-persona.md`).
- Check snippets for conflicts before pasting. `default_to_action` and `do_not_act_before_instructions` are mutually exclusive; reducing-verbosity fights state-tracking persistence.
