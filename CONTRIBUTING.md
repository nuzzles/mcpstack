# Contributing

Branch from `main`, keep changes focused, and open a pull request against `main`.
Use a descriptive branch name and a conventional commit/PR title, such as
`feat: export a stack` or `fix: preserve existing configuration`.

Before opening a PR, run the same checks as CI:

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --locked
```

Keep commands predictable for humans and agents. Preserve unrelated client
settings, never log secrets, and test meaningful behavior as it is implemented.
Do not commit real client configurations or credentials as fixtures.

## Changing the MSRV

Update `package.rust-version` in Cargo.toml, the README's current MSRV statement,
and the current version's row in its compatibility table. Preserve rows for
previous released versions when adding a new version. The dedicated MSRV CI job
reads Cargo.toml automatically and runs `cargo check --locked` with the declared
toolchain.

Unless explicitly stated otherwise, contributions are dual licensed under
MIT OR Apache-2.0, without additional terms or conditions.
