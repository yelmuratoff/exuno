# Publishing skills to claude.ai, ChatGPT, and Codex

claude.ai, ChatGPT, and the Codex app can't read the folders `exuno sync` writes on your machine. They install plugins from a marketplace in a git repository. The `plugin` tool writes that marketplace into your project: a plugin folder with your skills, commands, and subagents, and the index file each host looks for. You commit and push it, and each host pulls it from the repository.

## Turn it on

```bash
exuno enable plugin
exuno sync
```

Sync writes:

| Path | What it is |
| --- | --- |
| `plugin/skills/`, `plugin/commands/`, `plugin/agents/` | Your skills, commands, and subagents |
| `plugin/plugin.json` | The plugin manifest, in the [Agent Plugins](https://agent-plugins.org) 1.0 format |
| `.claude-plugin/marketplace.json` | The marketplace index claude.ai and ChatGPT read |
| `.agents/plugins/marketplace.json` | The marketplace index Codex reads |

claude.ai refuses a skill whose frontmatter has a key outside `name`, `description`, `license`, `compatibility`, `metadata`, and `allowed-tools`. The copies in `plugin/skills/` keep only those keys, and the commands in `plugin/commands/` keep only `description`. Sync prints a warning for each key it drops. The copies other tools get, such as `.claude/skills/`, keep every key.

None of these paths go into the managed `.gitignore` block, even with `outputs: local`, because the hosts read them from the repository. The manifests have no `version` field, so each host versions the plugin by commit.

## Name the plugin

The plugin is called `my-skills` until you rename it. Set your own values in `.ai/src/tools/plugin.yaml`; any you leave out keep the shipped default:

```yaml
targets:
  manifests:
    vars:
      name: "acme-skills"
      description: "Acme engineering skills"
      owner: "Acme"
```

Agent Plugins allows lowercase letters, digits, dots, and hyphens in `name`, up to 64 characters. A template that names a value you haven't set stops the sync with an error naming the template and the key, and the sync changes nothing.

To publish to fewer hosts, filter the templates. This skips the Codex index:

```yaml
targets:
  manifests:
    exclude: ".agents/*"
```

To change a template itself, put your version at the same path under `.ai/src/tools/plugin/manifests/`. That folder replaces the shipped set, so copy all three files, then edit the ones you need.

## Publish

Commit `plugin/`, `.claude-plugin/`, and `.agents/plugins/`, then push. Exuno never commits or pushes for you, so each push is the release.

**claude.ai.** In Customize › Plugins, choose Add marketplace, pick the GitHub repository, and turn on Sync automatically. For a private repository, install the Claude GitHub App on it first.

**ChatGPT.** A workspace admin can import the same `.claude-plugin/marketplace.json`. OpenAI doesn't document this for Plus or Pro accounts.

**Codex.** Add the repository once, then upgrade after each push:

```bash
codex plugin marketplace add owner/repo
codex plugin marketplace upgrade
```

## Claude Code loads the skills twice

A plugin claude.ai syncs also loads in Claude Code as `<name>@synced`, next to the skills Exuno already writes to `.claude/skills/`. Turn it off in your Claude Code user settings:

```json
{
  "enabledPlugins": {
    "my-skills@synced": false
  }
}
```

If Exuno manages your home directory, add this to `~/.ai/src/tools/claude/settings.json` and run `exuno sync`. Otherwise edit `~/.claude/settings.json` directly.
