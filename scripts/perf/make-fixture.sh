#!/usr/bin/env bash
# Generator for a deterministic .ai/src of a fixed size — the pinned input of
# scripts/perf/bench.sh.
#
# The engine's cost scales with the number of source files, so a benchmark is
# only comparable across months if its input is pinned. This generator is that
# pin: same arguments, same tree, byte for byte, on any machine. Defaults land
# near the 392-file source the design spec's 0.35.2 timings were measured on.
#
# Usage: make-fixture.sh <dir> [--skills N] [--refs N] [--rules N] [--commands N]

set -euo pipefail

DEST=""
SKILLS=96
REFS=2
RULES=70
COMMANDS=30

while [[ $# -gt 0 ]]; do
    case "$1" in
        --skills)   SKILLS="$2";   shift 2 ;;
        --refs)     REFS="$2";     shift 2 ;;
        --rules)    RULES="$2";    shift 2 ;;
        --commands) COMMANDS="$2"; shift 2 ;;
        -*)
            echo "Error: unknown flag: $1" >&2
            exit 1
            ;;
        *)
            [[ -z "$DEST" ]] || { echo "Error: one destination only" >&2; exit 1; }
            DEST="$1"
            shift
            ;;
    esac
done

[[ -n "$DEST" ]] || { echo "Usage: make-fixture.sh <dir> [--skills N] [--refs N] [--rules N] [--commands N]" >&2; exit 1; }

SRC="$DEST/.ai/src"
mkdir -p "$SRC/rules" "$SRC/skills" "$SRC/commands"

# ~40 lines per file: close to the median of a real rule or skill, so the
# per-file read and transform costs are representative rather than trivial.
_body() {
    local title="$1" i
    printf '# %s\n\n' "$title"
    for i in 1 2 3 4 5 6; do
        printf '## Section %s\n\n' "$i"
        printf 'A paragraph of prose that stands in for guidance text, long\n'
        printf 'enough that reading and rewriting the file costs what a real\n'
        printf 'one costs. It says nothing on purpose.\n\n'
        printf -- '- A list item that mentions `code` and a path like `lib/x.sh`.\n'
        printf -- '- A second item, for shape.\n\n'
    done
}

i=1
while [[ $i -le $RULES ]]; do
    _body "Rule $i" > "$SRC/rules/rule-$i.md"
    i=$((i + 1))
done

i=1
while [[ $i -le $COMMANDS ]]; do
    _body "Command $i" > "$SRC/commands/command-$i.md"
    i=$((i + 1))
done

i=1
while [[ $i -le $SKILLS ]]; do
    mkdir -p "$SRC/skills/skill-$i"
    {
        printf -- '---\n'
        printf 'name: skill-%s\n' "$i"
        printf 'description: Generated skill %s, for measurement only.\n' "$i"
        printf -- '---\n\n'
        _body "Skill $i"
    } > "$SRC/skills/skill-$i/SKILL.md"
    j=1
    while [[ $j -le $REFS ]]; do
        mkdir -p "$SRC/skills/skill-$i/references"
        _body "Skill $i reference $j" > "$SRC/skills/skill-$i/references/ref-$j.md"
        j=$((j + 1))
    done
    i=$((i + 1))
done

_body "Generated project" > "$SRC/AGENTS.md"

# Every shipped tool, so a run exercises the full 13-tool fan-out.
{
    printf 'tools:\n'
    printf '  enabled:\n'
    printf -- '    - %s\n' amazonq antigravity claude cline codex copilot cursor \
        gemini junie kimi opencode windsurf zed
} > "$DEST/.ai/exuno.yaml"

printf '%s\n' "$(find "$SRC" -type f | wc -l | tr -d ' ') files in $SRC"
