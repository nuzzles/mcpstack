## Examples

```sh
mcpstack codex import work.yaml --dry-run --non-interactive -y
mcpstack codex import work.yaml
mcpstack codex import work.yaml --server example --non-interactive -y
```

Merges approved server additions and replacements, preserving target servers that
are absent from the stack. A replacement replaces the selected server's definition.
Existing permissions are preserved unless `--include-permissions` is supplied.

`--dry-run` summarizes approved changes without writes or backups, but still asks
for approvals unless `-y` is supplied. `-y` alone does not suppress missing-secret
prompts; use `--non-interactive -y` for unattended previews and applies.

All selected changes are validated before writes. Changed configurations receive
recovery copies; identical imports cause no rewrite or new backup.
See [failures and recovery](recovery.md) for transaction boundaries.
