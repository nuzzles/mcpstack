Check for or install the latest stable release from crates.io.

```sh
mcpstack update --check
mcpstack update
mcpstack update --non-interactive -y
```

`--check` only reports versions. Installation requires confirmation or `-y`,
Cargo, and a working Rust toolchain. Cargo installs are updated in their original
location, including custom `--root` paths. Development binaries and other package
managers must be updated through their original installation method.

After interactive `export`, `import`, and `use`, mcpstack checks at most daily
and prints update notices to stderr. Checks take at most 750 ms of network time;
offline failures are silent. Read-only commands, dry runs, redirected output,
`--non-interactive`, and `--quiet` skip automatic checks.

Disable automatic checks with `--skip-version-check` or
`MCPSTACK_SKIP_VERSION_CHECK=true`. Explicit `update --check` still works.
Updates are never installed during other commands.
