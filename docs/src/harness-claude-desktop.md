# Claude Desktop

Manage local STDIO servers in `claude_desktop_config.json`.

```sh
mcpstack claude-desktop list
mcpstack claude-desktop use work.yaml --dry-run
```

| Platform | Default config |
| --- | --- |
| macOS | `~/Library/Application Support/Claude/claude_desktop_config.json` |
| Windows | `%APPDATA%\Claude\claude_desktop_config.json` |

Use `--config <PATH>` to override; other platforms require it.
Restart Claude Desktop after applying changes.

Supports `command`, `args`, and `env`; preserves unrelated preferences.
Remote connectors, extensions, and approvals stay in the app. Permission flags,
working directories, timeouts, and remote transports are unsupported.

All local server names are included; `--server` selects exact names.
Exported `${VAR}` strings remain literal. Stacks work with other harnesses when
their settings are compatible.

[Claude Desktop configuration](https://modelcontextprotocol.io/docs/develop/connect-local-servers)
