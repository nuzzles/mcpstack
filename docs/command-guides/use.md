Replace the entire server set. **Servers absent from the stack are removed.**

```sh
mcpstack codex use work.yaml --dry-run --non-interactive
mcpstack codex use work.yaml --non-interactive -y
```

An empty stack clears all servers. Unrelated settings are preserved.
Permissions require `--include-permissions`; `--server` is not supported.
