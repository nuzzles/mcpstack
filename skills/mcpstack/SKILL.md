---
name: mcpstack
description: Manage MCP server configurations with mcpstack. Use when inspecting, exporting, sharing, importing, or switching MCP stacks for Codex or Claude Code.
---

# mcpstack

Use the CLI to manage server configuration; it does not install or run MCP servers.
For client commands, use `mcpstack <command>` to select a harness interactively.
For unattended runs, pass `--harness codex` or `--harness claude`; repeat it only
for `import`. Other client commands choose one harness. The namespaced
`mcpstack codex <command>` and `mcpstack claude <command>` forms remain available.
Claude Desktop is unsupported.

## Discover the installed interface

Run `mcpstack --schema` for command arguments, defaults, dependencies, and error
codes as JSON. Use `mcpstack <command> --help` for a focused reference.
If the CLI is missing, install with `cargo install --locked mcpstack` when installation
is within the user's request. The skill itself does not install the executable.

Respect a supplied `--config <PATH>` throughout discovery, preview, and apply.
Without it, Codex uses `$CODEX_HOME/config.toml` or `~/.codex/config.toml`;
Claude uses user-scope `~/.claude.json`. Project/local Claude scopes are unsupported.

## Choose the operation

| Intent | Command |
| --- | --- |
| Inspect configured servers | `mcpstack codex list` |
| Read a masked YAML stack | `mcpstack codex show --non-interactive` |
| Save a masked stack | `mcpstack codex export work.yaml --non-interactive` |
| Validate stack syntax | `mcpstack validate work.yaml` |
| Preview a merge | `mcpstack codex import work.yaml --dry-run --non-interactive -y` |
| Merge into existing servers | `mcpstack codex import work.yaml --non-interactive -y` |
| Preview a full stack switch | `mcpstack codex use work.yaml --dry-run --non-interactive` |
| Replace the server set | `mcpstack codex use work.yaml --non-interactive -y` |

Substitute `claude` for Claude Code. `import` preserves servers absent from the
stack; `use` removes them, and an empty stack clears all servers. `diff` previews
merges only; use `use --dry-run` to see removals. Unrelated settings are preserved.

Preview before applying, keeping the same target, selection, and permission flags.
Use `-y` for changes already authorized by the user; otherwise present the preview
for approval. `--non-interactive` prevents prompts but does not approve writes.
Existing export files need `--overwrite`; do not add it unless replacement is intended.

`list`, `show`, and `export` skip some harness-provided servers by default. Inspect
the command's `--include-*` and `--all` options when the user needs them. Do not
assume a default export captures every server before switching stacks.

## Preserve secrets and permissions

Use `--non-interactive` for masked exports. Detection is name-based, so review
before sharing; arbitrary secrets embedded in URLs or arguments may remain.
Do not use `--expose-secrets` for shared files.

In portable stacks, `{value_env: VARIABLE}` resolves a masked literal at import;
`{env: VARIABLE}` preserves runtime lookup by the harness. HTTP runtime references
can include `prefix: "Bearer "`. Native `config` entries use `{$env: VARIABLE}` for
import-time lookup. Keep existing bindings distinct; never invent credential values
or put them in CLI arguments. Missing required values fail in noninteractive mode.

Permissions and tool filters are opt-in with `--include-permissions`; `--all` does
not include them. Otherwise existing target policies are preserved. Unsupported
fields or conversions fail before writes; do not silently drop them to force a transfer.
Codex supports STDIO/HTTP; Claude also supports SSE/WebSocket. Header helpers are
preserved for the harness to execute, not executed by mcpstack.

## Read results

`show` emits YAML and `--schema` emits JSON on stdout. Other commands emit text;
diagnostics and prompts go to stderr. There is no JSON result mode for mutations.
Nonzero exits indicate failure; zero may mean success, cancellation, or no change.
Inspect the result before retrying. Repeating an identical import/use makes no
configuration writes or backups.

For schema details and recovery, read the [documentation index](https://nuzzles.github.io/mcpstack/llms.txt).
