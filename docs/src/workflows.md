# Everyday workflows

All client commands use `mcpstack <client> <operation>`, where the client is
`codex` or `claude`. `--config <PATH>` selects a specific client configuration
instead of the [default location](compatibility.md#configuration-locations).

## Save and share a stack

```sh
mcpstack codex export work.yaml --non-interactive
mcpstack validate work.yaml
```

`export` writes a YAML file; `show` prints the same stack format on stdout:

```sh
mcpstack codex show --non-interactive
```

Recognized credentials become references to locally supplied environment values.
In an interactive terminal, export can ask whether to mask each credential.
`--non-interactive` selects masking without prompts. Avoid `--expose-secrets` for
shared files because it includes literal credentials. Review the result before
committing it. See [Secrets and permissions](secrets-and-permissions.md).

## Review and merge

```sh
mcpstack claude diff work.yaml
mcpstack claude import work.yaml --dry-run
mcpstack claude import work.yaml
```

`diff` shows a detailed, redacted preview of the import. Its redacted patches are
for review, not patch application. `import` shows named additions and replacements,
then asks for approval. It preserves target servers absent from the imported stack.
A replacement replaces the selected server's complete definition, with existing
permission policy preserved unless permission handling is explicitly included.

Import dry runs still ask for server approvals. For an unattended preview, use:

```sh
mcpstack claude import work.yaml --dry-run --non-interactive -y
```

This fails rather than prompting if a required masked secret is missing.

## Select a few servers

```sh
mcpstack codex export selected.yaml --server github --server linear --non-interactive
mcpstack claude diff selected.yaml --server github
mcpstack claude import selected.yaml --server github
```

Names must exist and match exactly. `--server` works with `list`, `show`, `export`,
`diff`, and `import`. Unknown selections fail before writes. Normally skipped
servers still need their corresponding `--include-*` flag or `--all`.
`--all` does not widen an explicit selection or include permissions.

## Switch the entire setup

```sh
mcpstack codex use personal.yaml --dry-run
mcpstack codex use personal.yaml
```

**`use` removes every target server absent from the stack**, including servers
added outside mcpstack. An empty stack removes all configured servers in the
selected scope. Unrelated client settings are preserved. Review the `use --dry-run`
summary to see removals; `diff` previews an import and does not preview these removals.

`use` asks once before applying the whole switch. Its dry run needs no approval,
but can still prompt for missing secrets unless `--non-interactive` is supplied.
`use` does not accept `--server`.

## Repeat safely

With the same stack, resolved secret values, and target configuration, repeated
imports and switches are no-ops: no duplicate servers, rewrites, or new backups.
Changing a referenced secret changes the resolved configuration and may require
a write even if the YAML file is unchanged.

All approved changes are prepared before writing. A failure does not continue
installing or applying independent servers. See [Failures and recovery](recovery.md)
for backup behavior and transaction limits.
