List configured servers without running them.

```sh
mcpstack list
mcpstack list --harness codex --non-interactive
mcpstack codex list
```

Select exact names with repeated `--server <NAME>`. `--all` includes normally skipped
servers but does not widen an explicit selection.
