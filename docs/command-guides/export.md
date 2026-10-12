## Examples

```sh
mcpstack codex export work.yaml --non-interactive
mcpstack codex export selected.yaml --server example --non-interactive
mcpstack codex export with-permissions.yaml --include-permissions --non-interactive
```

Saves a portable YAML stack to a file. Noninteractive exports mask recognized
credentials and fail if the destination exists unless `--overwrite` is supplied.
Review the file before sharing: masking cannot recognize every secret in arbitrary values.

Permissions are excluded unless `--include-permissions` is supplied. Normally
skipped servers require their `--include-*` flag or `--all`.
Use [show](command-show.md) for YAML on stdout, or
[validate](command-validate.md) to check the resulting stack.
