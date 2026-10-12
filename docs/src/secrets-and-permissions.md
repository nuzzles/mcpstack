# Secrets and permissions

## Keep credentials local

Masked literal values use a `value_env` reference in portable stacks:

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

Supply `EXAMPLE_AUTHORIZATION` through your shell or secret manager before running
an unattended import. Its value must be the complete header, including `Bearer `
when required. mcpstack resolves it during import and writes the literal into the
client configuration. Interactive previews can request missing values through a
hidden prompt. Values are never passed as mcpstack command-line arguments.

For a runtime binding instead, a header can use `{env: EXAMPLE_TOKEN}` or
`{env: EXAMPLE_TOKEN, prefix: "Bearer "}`. The client resolves that variable when
it starts or uses the server; mcpstack preserves the binding. This differs from
`value_env`, which restores a masked literal during import.

Native `config` entries use `{"$env": "VARIABLE"}` for a masked literal.
An inline credential argument such as `--token=VALUE` may be masked as one
reference; in that case supply the complete argument as the environment value.

## What masking covers

Masking recognizes credential field names, credential environment-variable names,
authorization/cookie headers, and named credential arguments. It preserves
ordinary commands, URLs, arguments, and existing references. It does not guarantee
detection of secrets embedded in arbitrary URLs or unnamed values.

Use `--non-interactive` on export to mask recognized credentials without questions.
`--expose-secrets` intentionally exports their literal values. Review any stack
before sharing it. Applied configurations and recovery backups can contain secrets;
keep them in directories with appropriate permissions.

## Permissions are always opt-in

Both clients omit permission metadata by default. `--all` includes skipped servers,
not permissions. To capture and restore policy deliberately:

```sh
mcpstack claude export with-policy.yaml --include-permissions --non-interactive
mcpstack claude diff with-policy.yaml --include-permissions
mcpstack claude import with-policy.yaml --include-permissions
```

The same flag applies to Codex and to `use`. Without it, imported permission
metadata and tool filters are ignored and existing target policy is preserved.

Policies use local tool names, without a harness name or native server prefix:

```yaml
schema_version: 1
servers:
  example:
    transport: {type: http, url: "https://mcp.example.com/mcp"}
    permissions:
      allow: [read]
      ask: [write]
      deny: [delete]
```

`permissions: {}` explicitly clears captured decisions when included; omitting
the field leaves existing approval policy alone. See the [stack format](stack-format.md)
for additional decisions and [client compatibility](compatibility.md) for limits.

## Claude settings

Claude's MCP permission arrays live in a separate settings file. Use
`--permissions-config <PATH>` to select it when including permissions. The default
is `~/.claude/settings.json`; with an explicit `--config`, it is `settings.json`
beside that config. Only the selected settings file is in scope: managed, project,
and other inherited policies are not evaluated.

Permission previews show added, modified, and deleted rules and their target
settings file. Declining the final permission approval cancels the entire import,
including server changes already selected in its prompts.
