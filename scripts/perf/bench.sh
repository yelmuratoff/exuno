#!/usr/bin/env bash
# Wall-time measurement of the CLI's commands on a generated fixture.
#
# Usage: bench.sh [--runs N] [--engines native|bash|both] [--keep]
#
# Prints a Markdown table of best and median wall time per command. The Bash
# engine was retired in Phase 6 of the Rust migration; its rows need a
# checkout of the last release that shipped it, pointed at by AGENTSYNC_BASH_CLI:
#
#   git worktree add ../agentsync-bash 0.37.0
#   AGENTSYNC_BASH_CLI=../agentsync-bash/bin/agentsync.sh bench.sh --engines both

set -euo pipefail

RUNS=3
ENGINES="native"
KEEP=false

while [[ $# -gt 0 ]]; do
    case "$1" in
        --runs)    RUNS="$2";    shift 2 ;;
        --engines) ENGINES="$2"; shift 2 ;;
        --keep)    KEEP=true;    shift ;;
        *) echo "Error: unknown flag: $1" >&2; exit 1 ;;
    esac
done

REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
BINARY="$REPO_DIR/target/release/exuno"
[[ -x "$BINARY" ]] || BINARY="$REPO_DIR/target/release/exuno.exe"
[[ -x "$BINARY" ]] || {
    echo "Error: build the engine first: cargo build --release" >&2
    exit 1
}

BASH_CLI="${AGENTSYNC_BASH_CLI:-}"
BASH_HOME=""
if [[ "$ENGINES" != "native" ]]; then
    [[ -f "$BASH_CLI" ]] || {
        echo "Error: --engines $ENGINES needs AGENTSYNC_BASH_CLI, the bin/agentsync.sh of a 0.37.0 checkout." >&2
        exit 1
    }
    BASH_HOME="$(cd "$(dirname "$BASH_CLI")/.." && pwd)"
fi

command -v /usr/bin/time >/dev/null 2>&1 || {
    echo "Error: /usr/bin/time is required for sub-second timing." >&2
    exit 1
}

FIXTURE="$(mktemp -d "${TMPDIR:-/tmp}/exuno_bench.XXXXXX")"
cleanup() { [[ "$KEEP" == "true" ]] || rm -rf "$FIXTURE"; }
trap cleanup EXIT INT TERM HUP

bash "$REPO_DIR/scripts/perf/make-fixture.sh" "$FIXTURE" >&2
git -C "$FIXTURE" init --quiet
git -C "$FIXTURE" config user.email bench@example.com
git -C "$FIXTURE" config user.name Bench

# One run of a command, in seconds with two decimals, from /usr/bin/time -p.
# Only the command's stdout is discarded: `time` reports on stderr, so the
# group's stderr is what carries the measurement.
_time_once() {
    local out
    out=$( { /usr/bin/time -p "$@" >/dev/null; } 2>&1 )
    printf '%s\n' "$out" | /usr/bin/awk '/^real/ { print $2 }'
}

# A sample set that does not parse is a silent zero, so prove the timer reads a
# known duration before any measurement is reported as a finding.
_self_check() {
    local seen
    seen=$(_time_once sleep 1)
    case "$seen" in
        1.0*|0.9*) return 0 ;;
        *)
            echo "Error: timer self-check failed: sleep 1 measured as '${seen:-nothing}'." >&2
            exit 1
            ;;
    esac
}

# Best and median of RUNS runs, as "best median".
_measure() {
    local i samples=""
    for ((i = 0; i < RUNS; i++)); do
        samples="$samples $(_time_once "$@")"
    done
    printf '%s\n' "$samples" | /usr/bin/awk '
        { for (i = 1; i <= NF; i++) v[n++] = $i + 0 }
        END {
            for (i = 0; i < n - 1; i++)
                for (j = 0; j < n - 1 - i; j++)
                    if (v[j] > v[j+1]) { t = v[j]; v[j] = v[j+1]; v[j+1] = t }
            printf "%.2f %.2f\n", v[0], v[int(n/2)]
        }'
}

_row() {
    local label="$1"
    shift
    local bash_result native_result
    bash_result=""
    native_result=""
    if [[ "$ENGINES" != "native" ]]; then
        bash_result=$(_measure env AGENTSYNC_NATIVE=0 AGENTSYNC_HOME="$BASH_HOME" \
            AGENTSYNC_REPO_ROOT="$FIXTURE" bash "$BASH_CLI" "$@")
    fi
    if [[ "$ENGINES" != "bash" ]]; then
        native_result=$(_measure env EXUNO_REPO_ROOT="$FIXTURE" "$BINARY" "$@")
    fi
    printf '| %s | %s | %s |\n' "$label" "${bash_result:-—}" "${native_result:-—}"
}

_self_check

# sync writes; every later measurement runs against an already-synced project,
# which is the state a user is in most of the time.
env EXUNO_REPO_ROOT="$FIXTURE" "$BINARY" sync >/dev/null 2>&1

printf '\n'
printf 'Fixture: %s\n' "$(find "$FIXTURE/.ai/src" -type f | wc -l | tr -d ' ') source files, 13 tools enabled"
printf 'Runs per cell: %s (best median, seconds)\n\n' "$RUNS"
printf '| Command | Bash | Native |\n'
printf '| --- | --- | --- |\n'
_row 'list' list
_row 'check' check
_row 'sync' sync
_row 'sync --if-stale' sync --if-stale
printf '\n'
