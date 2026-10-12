# Getting started

## Build from source

mcpstack is unpublished. Install [Rust](https://www.rust-lang.org/tools/install)
at or above the version declared in the repository's
[Cargo.toml](https://github.com/nuzzles/mcpstack/blob/main/Cargo.toml), then run:

```sh
git clone https://github.com/nuzzles/mcpstack.git
cd mcpstack
cargo install --path . --locked
mcpstack --version
```

Make sure Cargo's binary directory is on your `PATH`. From a checkout, you can also
replace `mcpstack` with `cargo run --locked --` in the commands below.
macOS, Linux, and Windows are covered by CLI integration tests.

## Try a preview with a scratch configuration

Create a new directory for this walkthrough. Save this as `demo.yaml`, or
[download the example](examples/demo.yaml):

```yaml
schema_version: 1
servers:
  example:
    transport:
      type: http
      url: https://mcp.example.com/mcp
```

The endpoint is illustrative. Validation and previews will not connect to it.

```sh
mcpstack validate demo.yaml
mcpstack codex use demo.yaml --config scratch.toml --dry-run --non-interactive
mcpstack claude use demo.yaml --config scratch.json --dry-run --non-interactive
```

Use new, unused filenames for `scratch.toml` and `scratch.json`. Missing target files
are accepted by these previews. No configuration or backup files are created.
You will see the target path and the server that would be added.

To apply the example to the scratch files:

```sh
mcpstack codex import demo.yaml --config scratch.toml --non-interactive -y
mcpstack claude import demo.yaml --config scratch.json --non-interactive -y
mcpstack codex list --config scratch.toml
mcpstack claude list --config scratch.json
```

Repeat the imports: identical entries do not cause another write or backup.
No server software is run by these commands.

## Work with your real setup

First inspect what is configured and export a masked copy:

```sh
mcpstack codex list
mcpstack codex export work.yaml --non-interactive
```

Replace `codex` with `claude` for Claude Code. Export fails if the destination
already exists in noninteractive mode; choose another name or deliberately add
`--overwrite`. Review the exported file before sharing it: masking recognizes
credential names, but cannot detect every secret embedded in arbitrary values.

Next, follow [Everyday workflows](workflows.md) to compare or apply a stack.
