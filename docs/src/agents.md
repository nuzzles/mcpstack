# For agents

[Documentation index](llms.txt) · [All Markdown](llms-full.txt) · [CLI schema](cli-schema.json)

```sh
mcpstack --schema
mcpstack codex import work.yaml --dry-run --non-interactive -y
mcpstack codex import work.yaml --non-interactive -y
```

Apply only after authorization. Use the installed binary's schema, explicit `--config`
paths when needed, and identical flags for preview/apply. For stack switches, preview
with `use --dry-run`; it shows removals that `diff` does not.

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
