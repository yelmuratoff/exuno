---
description: Bump version, update CHANGELOG.md, and prepare a release
argument-hint: "<major|minor|patch>"
---

Prepare a release for Exuno with version bump type: $ARGUMENTS (default: patch).

## Current State

!`cat VERSION`

!`git status --porcelain`

!`git log --oneline $(git describe --tags --abbrev=0 2>/dev/null || echo HEAD~20)..HEAD`

## What to do

Follow the `release` skill end to end — it owns the pre-release checks, the
CHANGELOG format, and the gotchas. Use the blocks above as its inputs: the
current version, the working tree the clean-tree check reads, and the commit
range the new CHANGELOG section must cover.

Stop after the release commit. Pushing and tagging belong to the user and CI.
