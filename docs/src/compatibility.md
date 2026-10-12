# Compatibility

mcpstack 0.1.0 supports **Codex** and **Claude Code**. A harness is the application
that loads and runs your MCP servers. Choose it with `mcpstack codex` or
`mcpstack claude`; the stack file itself is independent of the harness.

| Harness | Supported scope | Guide |
| --- | --- | --- |
| Codex | MCP servers in the selected `config.toml` | [Codex](harness-codex.md) |
| Claude Code | User-scope MCP servers and opt-in MCP permission rules | [Claude Code](harness-claude.md) |

Claude Desktop, Cursor, Gemini CLI, OpenCode, VS Code/Copilot, and Windsurf are
not supported. Claude Code project and local MCP scopes are not managed.

## Configuration locations

| Target | Default |
| --- | --- |
| Codex MCP configuration | `$CODEX_HOME/config.toml`, otherwise `~/.codex/config.toml` |
| Claude Code user MCP configuration | `~/.claude.json` |
| Claude Code permission settings | `~/.claude/settings.json` |

All client commands accept `--config <PATH>`. For Claude permissions,
`--permissions-config <PATH>` overrides the settings file. With an explicit
Claude config path, the default settings file is `settings.json` beside it.
Config filenames passed on the command line are relative to the current directory.

## Representable configuration

| Capability | Codex | Claude Code |
| --- | --- | --- |
| STDIO | Supported | Supported |
| HTTP | Supported | Supported |
| SSE | Rejected | Supported |
| WebSocket | Rejected | Rejected |
| Per-server working directory / timeouts | Supported fields follow the bundled Codex schema | Rejected |
| Shared permissions | Representable Codex policies | Representable Claude MCP rules |

The stack schema can describe more than either adapter supports. Imports and
previews reject unrepresentable fields before writes. MCP field validation is
offline and does not require either client to be installed.

## Permission conversion limits

Codex maps allow/ask to approve/prompt, keeps auto/writes, and maps exact denials
to disabled tools. It rejects partial tool patterns, wildcard denials, and
overlapping approval decisions it cannot preserve.

Claude rejects auto/writes decisions and default-ask policies with allow overrides.
When handling permissions, server names cannot overlap at a `__` separator, such
as `a` and `a__b`, because native rule prefixes would be ambiguous. Rename one
server first. Supported round trips preserve permission behavior, not exact
JSON/TOML spelling or layout. No unsupported decision is silently weakened.

## Skipped and reserved servers

Codex list/show/export skip `computer-use` and `node_repl` by default. Use
`--include-computer-use`, `--include-node-repl`, or `--all` to include them.

Claude skips `workspace`, `claude-in-chrome`, `computer-use`, `Claude Preview`,
and `Claude Browser`. Corresponding `--include-*` flags and `--all` allow inspection
and export, but Claude import/use reject those reserved names because Claude Code
ignores user definitions under those names. Rename them before applying a stack.

`--all` does not include permissions. That always requires `--include-permissions`.
