# <img src="docs/theme/favicon.svg" width="32" height="32" alt=""> mcpstack

[![crates.io](https://img.shields.io/crates/v/mcpstack.svg)](https://crates.io/crates/mcpstack)
[![docs.rs](https://docs.rs/mcpstack/badge.svg)](https://docs.rs/mcpstack)
[![CI](https://github.com/nuzzles/mcpstack/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/nuzzles/mcpstack/actions/workflows/ci.yml)

A Rust CLI for humans and agents to install multiple MCP servers, version-control
and switch stacks, and share setups across teams.

[Documentation](https://nuzzles.github.io/mcpstack/)

> [!NOTE]
> mcpstack is in early development.

## Harness Support

| Harness | mcpstack 0.1.0 | main (source) |
| --- | --- | --- |
| Codex | ✅ | ✅ |
| Claude Code | ✅ | ✅ |
| Claude Desktop | ❌ | ❌ |
| Cursor | ❌ | ❌ |
| Gemini CLI | ❌ | ❌ |
| OpenCode | ❌ | ❌ |
| VS Code/Copilot | ❌ | ❌ |
| Windsurf | ❌ | ❌ |

✅ Supported · 🚧 In progress · ❌ Not implemented

## Development

```sh
cargo run
```

## MSRV

This crate's [Minimum Supported Rust Version (MSRV)][MSRV] is currently **1.98**.

| mcpstack | Minimum Rust version |
| --- | --- |
| 0.1.0 | 1.98 |

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
