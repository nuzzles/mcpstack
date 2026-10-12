# Compatibility

mcpstack **0.1.0** supports:

| Harness | Scope | Transports |
| --- | --- | --- |
| [Claude Code](harness-claude.md) | User configuration | STDIO, HTTP, SSE |
| [Codex](harness-codex.md) | Selected config.toml | STDIO, HTTP |

Unsupported settings and permission conversions fail before writes.

Claude Desktop, Cursor, Gemini CLI, OpenCode, VS Code/Copilot, Windsurf,
Claude project/local scopes, and WebSocket transports are not supported.
