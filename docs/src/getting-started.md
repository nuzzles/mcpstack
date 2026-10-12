# Installation

Requires [Rust 1.98+](https://www.rust-lang.org/tools/install).

```sh
git clone https://github.com/nuzzles/mcpstack.git
cd mcpstack
cargo install --path . --locked
mcpstack --version
```

Keep Cargo's binary directory on your `PATH`. These docs cover **0.1.0**.

## Try it

[Download demo.yaml](examples/demo.yaml), then preview against an unused config path:

```sh
mcpstack validate demo.yaml
mcpstack codex use demo.yaml --config scratch.toml --dry-run --non-interactive
```

Nothing is written or executed. For Claude Code, use `claude` and `scratch.json`.
