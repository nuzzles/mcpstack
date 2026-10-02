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

Update only `package.rust-version` in Cargo.toml. The README refers to that field,
and the dedicated MSRV CI job reads it automatically and runs `cargo check --locked`
with the declared toolchain.

Unless explicitly stated otherwise, contributions are dual licensed under
MIT OR Apache-2.0, without additional terms or conditions.
