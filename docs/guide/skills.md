# Skills

A skill is a recipe the agent loads only when the task calls for it. Exuno uses the open [Agent Skills](https://agentskills.io) format: a folder with a `SKILL.md`, plus optional `references/`, `scripts/`, and `assets/` folders.

```
.ai/src/skills/
├── deploy/
│   └── SKILL.md
└── flutter/          # a category: no SKILL.md of its own
    └── bloc/
        └── SKILL.md
```

The `description` in the frontmatter decides when the agent picks the skill, so put the keywords first, mention the phrasings people use, and keep the domain narrow. The limit is 1024 characters. A `Gotchas` section helps the agent avoid repeating a mistake.

`exuno add skill deploy` creates a new skill from a template, and `exuno add skill bloc --category flutter` creates it inside a category.

## Categories

A folder under `skills/` without its own `SKILL.md` is a category, so `.ai/src/skills/flutter/bloc/SKILL.md` and `.ai/src/skills/backend/auth/SKILL.md` can sit side by side. Categories can nest up to four levels.

Tools disagree on nested skills (Claude Code, VS Code Copilot, and Gemini CLI skip them), so `sync` writes every skill flat at `<dest>/<name>/` for every tool. In the index written into an agents file, skills are grouped under a heading per category.

Because of that, a skill's name must be unique across all categories. Two skills with the same name stop `sync`. Category names follow the same lowercase-kebab rule as skill names, and `skills check` and `doctor` flag a folder that breaks it.

A tool's `targets.skills.include` and `exclude` match a skill's name or its category path, so `exclude: cloudflare/*` drops a whole category for one tool.

If you move a shipped skill such as `commit` into a category, `refresh` still finds it by name and updates it there.

## Inspecting skills

```bash
exuno skills list                 # every effective skill and its category
exuno skills show deploy          # description, category, source path, license, compatibility
exuno skills check                # missing or malformed metadata, bad categories, duplicate names
```

`skills list` reads the effective skill tree, including skills from a `shared:` parent and the built-in ones. `--profile <name>` inspects a profile, and `--include` and `--exclude` filter by name or category path. Of the problems `skills check` reports, only a duplicate name makes `sync` refuse to run.

The [Agent Skills specification](https://agentskills.io/specification) defines `name`, `description`, `compatibility`, and an optional `metadata` map of strings. For your own skills, that map can carry extra notes for people reading a skill card:

```yaml
metadata:
  exuno-use-when: Review a selected diff before merging
  exuno-not-for: Writing or fixing the code under review
  exuno-requirements: A selected diff and access to the repository
```

`skills show` labels these as **unverified annotations**. Exuno doesn't check them, doesn't act on them, and doesn't pick a skill based on them. The built-in check covers the required fields and common scalar forms; for full validation use the [reference validator](https://agentskills.io/specification#validation) (`skills-ref validate <skill-dir>`). [OASF](https://github.com/agntcy/oasf) is a separate capability taxonomy, and Exuno doesn't derive OASF mappings from a skill's description.

To look at skills outside this project, `exuno skills catalog list` and `show` read a catalog you choose and maintain by hand. See [skill cards](../skill-cards.md).

## The built-in `exuno` skill

The `exuno` skill teaches the agent how Exuno works. It ships inside the binary, built from `lib/templates/base-src/skills/`, so it matches the installed version instead of going stale in every project. Sync adds it after any `shared:` parent, so the order is: your project, then the shared parent, then the engine. When you update Exuno, the next `sync` in every project writes the new version, with no prompt and no merge.

To add to it and still get updates, create a folder named `exuno` without a `SKILL.md`, at `.ai/src/skills/exuno/` or inside any category. Its files join the engine's, and a file at the same path as one of the engine's replaces it. A `SKILL.append.md` there is appended to the engine's `SKILL.md` during sync, so the agent learns about your additions; it never reaches a tool as a separate file. The same works for a skill that a `shared:` parent provides.

To replace it completely, put your own `SKILL.md` in that folder. A project copy with its own `SKILL.md` always wins, and engine updates stop reaching it. To drop the skill altogether, set `base_skills: false` in `.ai/exuno.yaml`.

Projects created before the skill moved into the binary have their own copy, which hides the engine's. `exuno migrate` reports it, and `exuno migrate --apply` removes the copy if you haven't edited it. An edited copy stays, since it's a deliberate override.

## Hiding a personal skill

Tools also read personal skills from your home folder (`~/.claude/skills/`, `~/.agents/skills/`, `~/.config/opencode/skills/`), and none of them lets the project's copy of a name win. Claude Code runs the personal one, Codex lists both, and OpenCode doesn't document which wins. To replace a personal skill in one project, give the project skill its own name and hide the personal one:

```yaml
# .ai/exuno.yaml
skills:
  hide:
    - bloc        # personal skill this project replaces with .ai/src/skills/acme-bloc/
```

`exuno sync` then sets `skillOverrides.bloc: "off"` in `.claude/settings.json` and `permission.skill.bloc: "deny"` in `opencode.json`. Codex can turn a skill off only by its path, so Exuno writes nothing for it and Codex still lists both.

The entries contain only names, so a committed config works the same for every teammate and does nothing for someone who doesn't have that skill. Sync refuses to hide a name that a project skill also uses, since hiding works by name, and `exuno adopt` won't copy those entries back into the settings source. `exuno doctor` tells you when a project skill has the same name as a personal skill on your machine.
