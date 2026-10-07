---
description: Review the current branch diff for issues before merging
---

## Changed Files

!`git diff --name-only main...HEAD`

## Diff

!`git diff main...HEAD`

## What to do

Review the diff above by following the `review` skill — it owns the priority
order, the Exuno-specific checks, the output format, and the gotchas.

Report every finding, uncertain and low-severity ones included, with the exact
file and line, severity, and a concrete fix. If the code is solid, say so plainly.
