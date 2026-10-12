Merge server additions and replacements; preserve servers absent from the stack.

```sh
mcpstack codex import work.yaml --dry-run --non-interactive -y
mcpstack codex import work.yaml --non-interactive -y
```

`-y` approves changes; `--non-interactive` prevents secret prompts.
Codex and Claude Code preserve permissions unless `--include-permissions` is supplied.
