# Secrets and permissions

## Secrets

Use `--non-interactive` with `show` or `export` to mask recognized credentials.
Review shared files: arbitrary embedded secrets may escape detection.
`--expose-secrets` includes literals.

```yaml
schema_version: 1
servers:
  example:
    transport:
      type: http
      url: https://mcp.example.com/mcp
      headers:
        Authorization: {value_env: EXAMPLE_AUTHORIZATION}
```

Supply the complete header through `EXAMPLE_AUTHORIZATION`. `value_env` resolves at
import; `{env: TOKEN, prefix: "Bearer "}` preserves client runtime lookup instead.
Native `config` entries use `{$env: VARIABLE}` for masked literals.
Detected credentials inside header-helper commands mask the whole command with
`{value_env: VARIABLE}`; supply the complete command when importing.

## Permissions

Both harnesses require **`--include-permissions`** to export, compare, or apply policy.
Otherwise target policy is preserved. `--all` does not include permissions.

Use local tool names: `permissions: {allow: [read], ask: [write], deny: [delete]}`.
When included, `{}` clears captured decisions; an omitted policy preserves them.
Unsupported conversions fail before writes. See [compatibility](compatibility.md).
