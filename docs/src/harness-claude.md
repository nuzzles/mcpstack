# Claude Code

Supports STDIO, HTTP, and SSE in user configuration.

```sh
mcpstack claude list
```

| File | Default | Override |
| --- | --- | --- |
| MCP config | `~/.claude.json` | `--config` |
| Permissions | `~/.claude/settings.json` | `--permissions-config` |

With an explicit config path, permissions default to `settings.json` beside it.
Only the selected files are managed; inherited policies are not evaluated.

`list`, `show`, and `export` skip `workspace`, `claude-in-chrome`, `computer-use`,
`Claude Preview`, and `Claude Browser`. Use their `--include-*` flags or `--all` to
inspect/export them. Import/use reject these reserved names.

Permissions reject `auto`, `writes`, default-ask with allow overrides, and server
names ambiguous at `__` separators. Per-server working directories and timeouts are unsupported.
