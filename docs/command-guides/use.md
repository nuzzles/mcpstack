Replace the entire server set. **Servers absent from the stack are removed.**

```sh
mcpstack codex use work.yaml --dry-run --non-interactive
mcpstack codex use work.yaml --non-interactive -y
mcpstack codex use https://example.com/work.yaml --dry-run --non-interactive
```

An empty stack clears all servers. Unrelated settings are preserved.
Permissions require `--include-permissions`; `--server` is not supported.
HTTP(S) stack URLs work in both clients. Remote stacks cannot use stack-relative
commands or working directories; use absolute paths or `path_resolution: client`.
