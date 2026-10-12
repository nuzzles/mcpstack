## Examples

```sh
mcpstack codex show --non-interactive
mcpstack codex show --server example --non-interactive
mcpstack codex show --include-permissions --non-interactive
```

Prints a portable YAML stack to stdout; diagnostics remain on stderr.
Recognized credentials are masked in noninteractive mode. Interactive terminals
can ask whether to mask individual credentials. `--expose-secrets` includes literal
credentials, so avoid it for shared or captured output.

Permissions are excluded unless `--include-permissions` is supplied. Normally
skipped servers require their `--include-*` flag or `--all`.
Use [export](command-export.md) to save directly to a file.
