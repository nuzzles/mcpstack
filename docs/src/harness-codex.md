# Codex

```sh
mcpstack codex list
```

Reads `$CODEX_HOME/config.toml`, otherwise `~/.codex/config.toml`.
Override with `--config <PATH>`. Unrelated settings and comments are preserved.

`list`, `show`, and `export` skip `computer-use` and `node_repl`. Include them with
`--include-computer-use`, `--include-node-repl`, or `--all`.

Permissions map allow/ask to approve/prompt, retain auto/writes, and map exact
denials to disabled tools. Partial tool patterns, wildcard denials, and overlapping
approval decisions are rejected. Native fields follow the bundled Codex schema.
