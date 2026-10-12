# Claude Code

Use `mcpstack claude <command>` to manage Claude Code's **user-scope** MCP servers.
Claude Desktop and Claude Code project/local scopes are not supported.

## Configuration

The default MCP configuration is `~/.claude.json`. Pass `--config <PATH>` to
target another JSON file. Unrelated configuration is preserved during changes.

Permission rules live in `~/.claude/settings.json`. With an explicit MCP config
path, mcpstack defaults to `settings.json` beside that file. Commands that handle
permissions accept `--permissions-config <PATH>` to override this location.

```sh
mcpstack claude list
mcpstack claude export work.yaml --non-interactive
mcpstack claude diff work.yaml --config target.json --non-interactive
```

## Commands

| Command | Purpose |
| --- | --- |
| [list](command-list.md#mcpstack-claude-list) | Inspect configured server names and transports. |
| [show](command-show.md#mcpstack-claude-show) | Print a portable YAML stack. |
| [export](command-export.md#mcpstack-claude-export) | Save a portable YAML stack to a file. |
| [diff](command-diff.md#mcpstack-claude-diff) | Review a redacted import diff. |
| [import](command-import.md#mcpstack-claude-import) | Merge approved server additions and replacements. |
| [use](command-use.md#mcpstack-claude-use) | Replace the entire configured server set. |

See [Commands](cli-reference.md) for global options and exit codes.

## Supported settings

Claude Code supports STDIO, HTTP, and SSE transports. WebSocket definitions and
per-server working-directory or timeout settings are rejected.

MCP permission rules are excluded from exports by default and preserved on import/use.
Add `--include-permissions` to explicitly transfer them. Rules use the shared stack
permission format; unsupported policies fail before writes. See
[conversion limits](compatibility.md#permission-conversion-limits) and
[recovery](recovery.md) for changes spanning the MCP and permission files.

## Reserved servers

`list`, `show`, and `export` skip `workspace`, `claude-in-chrome`, `computer-use`,
`Claude Preview`, and `Claude Browser`. Their respective `--include-*` flags or
`--all` include them for inspection and export.

`import` and `use` reject these reserved names because Claude Code ignores user
definitions under them. Rename them in a stack before applying it.
`--all` does not include permissions or widen an explicit `--server` selection.
