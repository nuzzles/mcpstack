## Examples

```sh
mcpstack codex list
mcpstack codex list --server example
mcpstack codex list --all
```

Lists server names and transports without executing servers or checking connections.
`--config <PATH>` selects an existing configuration file. Repeat `--server <NAME>`
to select exact names; unknown names fail.

Normally skipped servers require the corresponding `--include-*` flag or `--all`.
An explicit selection remains in effect with `--all`. To inspect full definitions
as YAML, use [show](command-show.md).
