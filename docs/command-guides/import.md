Merge server additions and replacements; preserve servers absent from the stack.

```sh
mcpstack codex import work.yaml --dry-run --non-interactive -y
mcpstack codex import work.yaml --non-interactive -y
```

`-y` approves changes; `--non-interactive` prevents secret prompts.
Permissions are preserved unless `--include-permissions` is supplied.
