# mcpstack

A Rust CLI for humans and agents to install multiple MCP servers, version-control
stacks, and share setups across teams.

## Development

```sh
cargo run
```

See [CONTRIBUTING.md](CONTRIBUTING.md) for checks and contribution guidelines.
CI runs on pushes and pull requests. There is no release or deployment workflow.

## MSRV

This crate's [Minimum Supported Rust Version (MSRV)][MSRV] is declared by
`package.rust-version` in Cargo.toml.

CI checks compilation with this toolchain separately from the latest stable Rust
checks. See [CONTRIBUTING.md](CONTRIBUTING.md#changing-the-msrv) for how to change
the MSRV.

[MSRV]: Cargo.toml

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.
