# Claude Code

Supports STDIO, HTTP, SSE, and WebSocket in user configuration.

```sh
mcpstack claude list
```

| File | Default | Override |
| --- | --- | --- |
| MCP config | `~/.claude.json` | `--config` |
| Permissions | `~/.claude/settings.json` | `--permissions-config` |

With an explicit config path, permissions default to `settings.json` beside it.
Only the selected files are managed; inherited policies are not evaluated.

WebSocket stacks use `transport: {type: websocket, url: "wss://example.com/events"}`;
Claude's native entry uses `type: "ws"`. Both `ws://` and `wss://` are supported.
Authentication uses headers, with the same masking and environment references as
HTTP. OAuth and dynamic `headersHelper` commands are not supported for WebSocket.

`list`, `show`, and `export` skip `workspace`, `claude-in-chrome`, `computer-use`,
`Claude Preview`, and `Claude Browser`. Use their `--include-*` flags or `--all` to
inspect/export them. Import/use reject these reserved names.

Permissions reject `auto`, `writes`, default-ask with allow overrides, and server
names ambiguous at `__` separators. Per-server working directories and timeouts are unsupported.
