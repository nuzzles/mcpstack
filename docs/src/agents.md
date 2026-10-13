# For agents

[Documentation index](llms.txt) · [All Markdown](llms-full.txt) · [CLI schema](cli-schema.json)

## Install the skill

```sh
npx skills add https://nuzzles.github.io/mcpstack
```

Or install from GitHub: `npx skills add nuzzles/mcpstack --skill mcpstack`.
Add `-g` for all projects or `-a codex -a claude-code` to select agents.

The [skill](https://nuzzles.github.io/mcpstack/.well-known/skills/mcpstack/SKILL.md) teaches agents the CLI workflow;
[install mcpstack](getting-started.md) separately.

## Automation

```sh
mcpstack --schema
mcpstack codex import work.yaml --dry-run --non-interactive -y
mcpstack codex import work.yaml --non-interactive -y
```

Apply only after authorization. Use the installed binary's schema, explicit `--config`
paths when needed, and identical flags for preview/apply. For stack switches, preview
with `use --dry-run`; it shows removals that `diff` does not.
Schema `requires` lists companion argument IDs needed when an option is supplied.

| Output | Format |
| --- | --- |
| `--schema` | JSON interface and exit codes |
| `show` | YAML stack |
| Other stdout | Human-readable text |
| stderr | Diagnostics and prompts |

`--non-interactive` disables prompts; `-y` approves changes. Missing secrets fail;
supply them through environment variables. Permissions require `--include-permissions`.
Nonzero exits mean failure; zero can also mean cancellation or no change. Inspect
results before retrying. There is no JSON result mode for imports or switches.
