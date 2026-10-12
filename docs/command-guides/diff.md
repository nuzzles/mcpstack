## Examples

```sh
mcpstack codex diff work.yaml --non-interactive
mcpstack codex diff work.yaml --server example --non-interactive
mcpstack codex diff work.yaml --include-permissions --non-interactive
```

Shows the detailed, redacted changes that an [import](command-import.md)
would make. It does not write configuration, create backups, or execute servers.
The output is for review and cannot be applied as a patch.

Missing masked secrets fail with `--non-interactive`; otherwise they may prompt.
Permission changes are compared only with `--include-permissions`.

An import preserves servers absent from the stack, so this diff does not preview
their removal. Use [use --dry-run](command-use.md) to preview a full switch.
