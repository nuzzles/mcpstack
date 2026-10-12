# Codex

Use `mcpstack codex <command>` to manage MCP servers in a Codex TOML configuration.

## Configuration

The default file is `$CODEX_HOME/config.toml` when `CODEX_HOME` is set, otherwise
`~/.codex/config.toml`. Pass `--config <PATH>` to target a different file.
Imports and stack switches preserve unrelated TOML settings and comments.

```sh
mcpstack codex list
mcpstack codex export work.yaml --non-interactive
mcpstack codex diff work.yaml --config target.toml --non-interactive
```

## Commands

| Command | Purpose |
| --- | --- |
| [list](command-list.md#mcpstack-codex-list) | Inspect configured server names and transports. |
| [show](command-show.md#mcpstack-codex-show) | Print a portable YAML stack. |
| [export](command-export.md#mcpstack-codex-export) | Save a portable YAML stack to a file. |
| [diff](command-diff.md#mcpstack-codex-diff) | Review a redacted import diff. |
| [import](command-import.md#mcpstack-codex-import) | Merge approved server additions and replacements. |
| [use](command-use.md#mcpstack-codex-use) | Replace the entire configured server set. |

See [Commands](cli-reference.md) for global options and exit codes.

## Supported settings

Codex supports STDIO and HTTP transports. SSE and WebSocket definitions are rejected.
Native `config` fields are checked against the bundled Codex configuration schema,
including supported working-directory and timeout fields.

Permissions and tool filters are excluded from exports by default and preserved
on import/use. Add `--include-permissions` to explicitly transfer them. Codex can
represent allow/ask decisions as approve/prompt, retain auto/writes, and map exact
denials to disabled tools. See [conversion limits](compatibility.md#permission-conversion-limits).

## Normally skipped servers

`list`, `show`, and `export` skip `computer-use` and `node_repl`. Add
`--include-computer-use`, `--include-node-repl`, or `--all` to include them.
`--all` does not include permissions or widen an explicit `--server` selection.
