Merge server additions and replacements; preserve servers absent from the stack.

```sh
mcpstack codex import work.yaml --dry-run --non-interactive -y
mcpstack codex import work.yaml --non-interactive -y
mcpstack codex import https://example.com/work.yaml --dry-run --non-interactive -y
```

`-y` approves changes; `--non-interactive` prevents secret prompts.
Permissions are preserved unless `--include-permissions` is supplied.
HTTP(S) stack URLs work in both clients. Remote stacks cannot use stack-relative
commands or working directories; use absolute paths or `path_resolution: client`.
