# Stack format

YAML with `schema_version: 1` and named servers. No source-harness labels.

```yaml
schema_version: 1
servers:
  local:
    transport: {type: stdio, command: ./bin/server}
  remote:
    transport: {type: http, url: "https://mcp.example.com/mcp"}
```

| Field | Purpose |
| --- | --- |
| `transport` | STDIO command/args/env, or HTTP/SSE/WebSocket URL and headers |
| `transport.headers_helper` | Shell command that returns headers as JSON; HTTP in both harnesses, SSE/WebSocket in Claude |
| `config` | Native server fields; use instead of `transport` |
| `settings` | Enabled/required flags, timeouts, tool filters |
| `permissions` | Local tool decisions: allow, ask, deny, auto, writes; optional default_mode |

Unsupported target fields fail. Tool filters and permissions require
`--include-permissions`. Deny takes precedence over ask, then allow; patterns depend
on the harness. See [compatibility](compatibility.md) and [secret references](secrets-and-permissions.md).

The harness runs header helpers using its own authentication precedence and refresh
rules. mcpstack preserves commands verbatim without running them; use `PATH` or absolute script paths.

## Paths

`path_resolution: stack` (default) resolves relative commands and `cwd` from the stack's
directory. Bare commands use `PATH`. Exports use `path_resolution: client` to retain
original runtime behavior. Arguments and environment strings are not expanded or rebased.

Check syntax with `mcpstack validate work.yaml`; preview against the target to check compatibility.
