# mcpstack

Switch MCP server stacks like nvm switches Node versions. Version-control stack
files and share setups across clients, humans, and agents.

> [!WARNING]
> mcpstack is in early development. See [FUNCTIONAL_REQUIREMENTS.md](FUNCTIONAL_REQUIREMENTS.md) for the feature checklist.

## Switch stacks

```sh
mcpstack use work.yml
mcpstack use personal.yml --dry-run
mcpstack use personal.yml -y
```

`use` replaces **all** MCP servers with the selected stack, removing servers absent
from it while preserving other client settings. It validates first, confirms the
whole switch, and creates a numbered backup before writing. `--dry-run` previews
without writes; `-y` approves the switch without prompting. An empty stack clears
all MCP servers. Currently targets Codex by default; use `--config <path>` for an
explicit config. It changes configuration without installing or starting servers.

## Development

```sh
cargo run
```

## MSRV

This crate's [Minimum Supported Rust Version (MSRV)][MSRV] is currently **1.98**.

| mcpstack | Minimum Rust version | Codex config | Claude Code version |
| --- | --- | --- | --- |
| 0.0.0 (unpublished) | 1.98 | Current MCP schema | Unsupported |

See [CONTRIBUTING.md](CONTRIBUTING.md#changing-the-msrv) for how to change
the MSRV.

[MSRV]: Cargo.toml

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for contribution guidelines.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.
