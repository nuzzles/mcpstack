Preview a redacted import diff without writing files or running servers.

```sh
mcpstack diff work.yaml --harness codex --non-interactive
mcpstack codex diff work.yaml --non-interactive
mcpstack codex diff https://example.com/work.yaml --non-interactive
```

Missing secrets fail in noninteractive mode. This previews merges;
use [use --dry-run](command-use.md) to preview removals during a stack switch.
