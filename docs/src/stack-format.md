# Stack format

Stacks are YAML files with `schema_version: 1` and a map of named servers. Each
server uses either a portable `transport` or a `config` object. The format has
no source-harness ownership field. The target adapter decides what it can represent.

```yaml
schema_version: 1
servers:
  local:
    transport:
      type: stdio
      command: ./bin/example-server
      args: ["--quiet"]
  remote:
    transport:
      type: http
      url: https://mcp.example.com/mcp
```

These are illustrative servers. mcpstack does not install or execute them.

## Portable definitions

STDIO supports `command`, `args`, `env`, forwarded `env_vars`, `cwd`, and
`path_resolution`. HTTP, SSE, and WebSocket describe a `url` and optional `headers`.
Not every client supports every transport or field; consult
[Client compatibility](compatibility.md).

Optional `settings` include `enabled`, `required`, positive startup/tool timeouts,
and tool filters. Unsupported target settings are rejected, not silently dropped.
Tool filters are exported and applied only with `--include-permissions`.

`value_env` supplies masked literals during import. Header `env` references retain
client runtime lookup. Read [Secrets and permissions](secrets-and-permissions.md)
before choosing between them.

## Paths

With the default `path_resolution: stack`, a relative command like `./bin/server`
is resolved from the directory containing the stack. A relative `cwd` is resolved
independently from that same directory. Bare commands such as `npx`, `uvx`, or
`docker` remain `PATH` lookups. Absolute paths remain unchanged.

Arguments, URLs, environment values, and `${...}` strings are not rebased or
shell-expanded. Use `/` for path separators in stacks shared across platforms.
Drive-relative Windows paths such as `C:server.exe` are rejected on Windows.

Exports use `path_resolution: client` to preserve existing command/cwd strings
with their original client runtime semantics. Use `stack` for relative paths
bundled alongside a hand-authored stack. Paths need not exist during validation.

## Shared permission decisions

`permissions` uses local tool names:

| Field | Meaning |
| --- | --- |
| `allow` | Approve matching tools. |
| `ask` | Require approval for matching tools. |
| `deny` | Block matching tools. |
| `auto` | Preserve automatic approval for exact tools where supported. |
| `writes` | Preserve write-sensitive approval for exact tools where supported. |
| `default_mode` | Fallback decision: `allow`, `ask`, `auto`, or `writes`. |

`allow`, `ask`, and `deny` may contain `'*'` or patterns where the target supports
them. Matching deny rules take precedence over ask, then allow. An ask wildcard
is different from an ask fallback: it also constrains explicit allow matches.

Omitted permission metadata preserves existing approval policy. Included empty
metadata clears captured decisions. Conversions must preserve behavior; unsupported
combinations fail. Legacy `source: claude`, `source: codex`, and native-qualified
`mcp__server__tool` rules are rejected. Re-export the native configuration with
`--include-permissions` to migrate.

## Configuration definitions

A `config` object can hold a server definition accepted by the target adapter:

```yaml
schema_version: 1
servers:
  example:
    config:
      command: example-server
      args: ["--quiet"]
```

Do not combine `config` and `transport` in one server. Exporters produce portable
transport entries. Schema validation checks the stack itself; a successful
`validate` does not promise that every client can represent it. Use a preview
against the intended client to check compatibility.
