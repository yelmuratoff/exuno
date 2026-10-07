# Evaluating & Trigger-Tuning Skills

The measurement half of the iteration loop. [`writing-skills.md`](writing-skills.md) tells you how to write a skill; this tells you how to prove it works and tune when it fires. Load when:

- A skill is high-stakes and you need it to beat the no-skill baseline, not just "look right".
- A skill under- or over-triggers and you want to tune the description against real queries.
- You're deciding whether a new version is actually better than the one it replaces.

This covers *is the output better and does the trigger fire*. Whether a discipline skill (TDD, verification) holds up when the agent is pressed to rationalize past it is a separate pressure test, run against scenarios built to tempt the shortcut.

## Contents

- The eval loop — with-skill vs. baseline
- Protecting the harness from repository contamination
- Discriminating assertions
- Reading the benchmark
- Token-cost claims — what the number has to be
- Improving without overfitting
- Tuning the trigger
- Is the new version actually better — blind comparison

## The eval loop — with-skill vs. baseline

When a skill's value is "better output on a task" (not "agent complies under pressure"), measure it against a baseline:

1. **Run with-skill AND baseline in the same pass.** For each test case spawn both runs at once so they finish together — launching the with-skill runs first and the baselines later wastes a turn.
   - New skill → baseline is *no skill* (same prompt, nothing loaded).
   - Improving an existing skill → baseline is the *old version*. Snapshot it first (`cp -r <skill> /tmp/skill-snapshot`) and point the baseline run at the snapshot, because you're about to overwrite the original.
2. **Grade each run against assertions** (below); save a structured result per run.
3. **Aggregate** across runs — pass-rate, time, tokens as mean ± stddev, plus the with-skill − baseline delta. Variance matters as much as the mean: a high-variance eval is flaky, not a clean signal.
4. **Review, improve, rerun.** Read the user's feedback, fix the skill, run a fresh iteration. Stop when the user is satisfied, the feedback is empty, or you stop making progress.

Skills improve through execution, not introspection — read the *transcripts*, not just final outputs. Wasted steps and dead-end branches mean an instruction is vague, doesn't apply, or offers options without a default.

The same loop measures any *context or tooling* addition, not only a `SKILL.md` — a project map injected into context, a CLI/MCP tool, a hook. Hold the harness, task set, and revision fixed and toggle only the addition; the baseline is the agent without it. To *shrink* an addition safely (a map or knowledge doc that costs too much context), freeze the task pool and iterate: compress ~30%, re-run, keep compressing while pass-rate holds, and stop at the first step that drops quality — revert to the prior version.

## Protect the experiment from its own repository

An agent eval is contaminated when the runner can discover the answer through a path the condition did not intend. A fresh session is insufficient if its working directory still inherits an ancestor `AGENTS.md`/`CLAUDE.md`, exposes the implementation source, or makes a repo-local binary reachable outside the instrumented path.

1. **Run outside the implementation tree.** Copy the minimal fixture into a scratch directory whose ancestors contain no project instructions or source checkout. Resolve symlinks and executable paths during preflight; walking upward or invoking a global/repo-root binary must not reveal the candidate capability.
2. **Use one fresh context per condition × task × repetition.** Make each task self-contained and keep conditions out of the same session so earlier discovery cannot leak into later runs.
3. **Instrument the behavior you claim to measure.** Treat tool-invocation logs, files, tests, or other observable artifacts as ground truth. Keep the agent's transcript/self-report as a secondary diagnostic signal, then compare the two — disagreement often exposes a bypass or a broken instrument.
4. **Prove a floor and ceiling before the full matrix.** The bare control should not discover the hidden capability; an explicit condition should. Inspect the actual rendered prompt, settings, working directory, and executable resolution when either control surprises you.
5. **Score completion and cost alongside adoption.** A condition that triggers the desired tool more often but leaves tasks unfinished or burns far more turns/tokens is not an unqualified win.

This is **harness contamination**, distinct from model-training contamination in `ai-evaluation`. The former is under your direct control and invalidates the experiment even when every score looks plausible.

## Evaluate across the model pool, not one model

A skill tuned on one model is not validated for the others. Trigger firing, tool use, and instruction-following all shift between models, so a with-skill win on your model says nothing about the rest — and if your team or a portable skill store spans several models, a single-model pass leaves most users on an unproven skill. Run the with-skill/baseline matrix across the main models actually in use, including the weaker and budget ones (they use tools markedly worse), and treat a skill that only wins on one model as un-validated.

## Discriminating assertions

An assertion is worth something only if it **passes when the skill genuinely succeeds and fails when it doesn't.** A passing grade on a weak assertion is worse than none — it manufactures false confidence.

- **Check substance, not surface.** "A file named `report.pdf` exists" passes for an empty file. Assert on content — the right value in the right cell, the extracted name matching the input.
- **No partial credit.** Each assertion is pass or fail, and the burden of proof is on the assertion: when uncertain, fail it.
- **Critique the evals while grading.** Flag any assertion that would also pass for a clearly wrong output, any important outcome no assertion covers, and any assertion you can't verify from the outputs. Keep the bar high — flag what the author would call a good catch, not nitpicks.
- **Verify the run's implicit claims too.** Beyond the predefined assertions, extract the factual/process/quality claims the output makes ("used pdfplumber", "all 12 fields filled") and check them against the artifacts. This catches what the assertions missed.
- **Prefer a script over eyeballing** for anything programmatically checkable — faster and reproducible across iterations.

## Reading the benchmark — patterns the averages hide

After aggregating, look past the headline pass-rate:

- **Non-discriminating** — passes in *both* with-skill and baseline. It isn't measuring the skill's value; sharpen it or drop it.
- **Always-fails-in-both** — the assertion is broken or the task is beyond current capability; fix the assertion before blaming the skill.
- **Flips the right way** — passes with skill, fails without. This is where the skill earns its keep; protect it.
- **Flips the wrong way** — passes without, fails with. The skill is *hurting* here; find the instruction that misleads the agent.
- **High variance** — same config, wildly different runs. Flaky eval or non-deterministic behavior; don't trust its mean.
- **Cost delta** — name it when the skill buys pass-rate at a real time/token cost, so the trade is explicit.

Report observations grounded in the data; leave skill *fixes* to the improvement step.

## Token-cost claims — what the number has to be

A context optimization — deferring a skill body, isolating a stage in a subagent, compressing a memory file — invites a headline number ("35% fewer tokens"). Counted tokens are not billed spend, and the gap runs in both directions. Before the number leaves your hands:

- **Price the prefix, don't count it.** A stable prefix that repeats across turns is billed as a cache read at roughly a tenth of base input, while writing that cache costs *more* than sending it uncached (multipliers and break-even in `ai-architecture` → `references/latency-and-cost.md`). Counting raw input tokens therefore overstates the money saved on a cached prefix by about an order of magnitude. A local tokenizer delta is a size measurement, never a spend measurement.
- **Check which direction the cache moved.** Read `cache_read_input_tokens`, `cache_creation_input_tokens`, and `input_tokens` separately rather than summing them. An "optimization" that injects fresh content into the system prefix each stage invalidates everything after it and forces a re-write every time — the token count falls while the bill rises. Zero cache reads across repeated requests means a silent invalidator, not a lean prompt.
- **Count both arms whole.** Subagent tokens, extra round-trips, and retries belong to the with-change arm. Loading one stage at a time adds a selector turn and a file read per stage, and each added turn re-sends the whole conversation — on a long pipeline that can outweigh the prefix it saved. Isolation reliably cuts *window pressure*; it does not automatically cut *combined spend*.
- **Keep the units honest.** Words, tokens, and money are three different quantities. A saving stated in words against a total stated in tokens is not a ratio until the conversion is named.
- **The A/B outranks the arithmetic.** Same task, same quality bar, both arms, N ≥ 3 runs, compared on provider-reported usage. When the arithmetic and the billed total disagree, the billed total wins.

**State the claim boundary.** Every optimization worth recommending should carry the claim it does *not* license — "this shrinks the window, it does not prove a lower bill", "this cuts main-agent tokens, measured before subagent cost". An optimization published without its boundary reads as a stronger result than the evidence supports, which is the failure `verification.md` exists to prevent.

## Improving without overfitting

You iterate on a handful of examples because they're fast to judge — but the skill will run on thousands of prompts you never see. Mind the generalization gap:

- **Generalize from each piece of feedback.** Don't bolt on a fiddly fix that satisfies only this example. If an issue is stubborn, branch out — a different metaphor or working pattern — rather than piling on rigid absolute clauses. Reframing and explaining *why* travels further than another constraint.
- **Keep the prompt lean.** Remove instructions that aren't pulling their weight. If the transcript shows the skill sending the agent down an unproductive path, cut the line that caused it and re-test.
- **Repeated work across runs → bundle a script.** If every baseline transcript independently reinvents the same helper (a parser, a chart-builder, a validator), that's the signal to write it once in `scripts/` and have the skill call it. See the Bundled scripts pattern in [`writing-skills.md`](writing-skills.md).

## Tuning the trigger

The description decides whether a future agent ever loads the skill — [`writing-skills.md`](writing-skills.md) covers *how to write* it. To *measure and tune* it:

1. **Build ~20 realistic trigger queries**, split should-trigger (8–10) and should-not-trigger (8–10). Make them concrete — file paths, real column names, a company name, a line of backstory, casual phrasing, the odd typo — not abstract ("Format this data").
2. **Spend the should-not-trigger budget on near-misses.** An obviously irrelevant query ("write a fibonacci function" for a PDF skill) tests nothing. The valuable negatives share keywords with the skill but actually need something else — adjacent domains, ambiguous phrasing a naive keyword match would wrongly fire on.
3. **Split train / held-out (~60/40), run each query a few times** (triggering is non-deterministic — one run lies), and **select the description that scores best on the held-out set, not on train** — optimizing on train overfits.

### How triggering actually works — why some queries can't be fixed

Agents consult a skill only for tasks they *can't* easily handle alone. A simple one-step query ("read this PDF") often won't trigger a skill even with a perfect description, because the agent just does it. So:

- Trigger eval queries must be **substantive** — multi-step or specialized enough that consulting the skill is worth it. Trivial queries are poor test cases regardless of description quality.
- You can't tune your way out of under-triggering on genuinely trivial tasks; that's the harness deciding the skill isn't needed, not a description bug.

## Is the new version actually better — blind comparison

When you need a rigorous "did the change help" rather than a feel, compare blind: hand both outputs (old vs. new) to an independent agent **without telling it which is which**, let it pick the better one against a task-derived rubric, then unblind and analyze *why* the winner won — which instruction, script, or example made the difference. Stripping the which-is-mine signal removes the bias that makes you grade your own new version generously. Heavier than the human-review loop and usually optional.
