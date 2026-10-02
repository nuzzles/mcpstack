# mcpstack

A Rust CLI for humans and agents to install multiple MCP servers, version-control
stacks, and share setups across teams.

> [!WARNING]
> mcpstack is in early development. See [FUNCTIONAL_REQUIREMENTS.md](FUNCTIONAL_REQUIREMENTS.md) for the feature checklist.

## Development

```sh
cargo run
```

## Agent interface

```sh
cargo run -- --schema
cargo run -- --json --non-interactive
cargo run -- --help
```

`--schema` emits schema version 1 as JSON, generated from the argument definitions
used for parsing and help. It includes commands, flags, types, defaults,
constraints, examples, and exit statuses. Its version is independent of stack
file versions. Import/export commands are not implemented yet.

`--json` emits one result (`{"ok":true,"result":...}`) or error
(`{"ok":false,"error":{"code":...,"message":...}}`) on stdout, including help
and version requests. `--schema` always emits JSON; invalid schema requests use
the error envelope. `--non-interactive` disables prompts; current operations do
not prompt in either mode.

Exit statuses: **0** success/help/version, **1** output failure (`OUTPUT_ERROR` on
stderr), **2** invalid arguments (`INVALID_ARGUMENT` in JSON mode). Output failures
cannot guarantee a JSON result because stdout may be unavailable. Machine-readable
argument errors omit input values to avoid exposing credentials.

## MSRV

This crate's [Minimum Supported Rust Version (MSRV)][MSRV] is currently **1.98**.

| mcpstack | Minimum Rust version |
| --- | --- |
| 0.0.0 (unpublished) | 1.98 |

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
