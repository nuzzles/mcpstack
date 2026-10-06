# Contributing

## Git Workflow

Never commit directly to `main`. Branch from `main` and open a pull request against
`main`, including for small changes.

### Branch Naming Convention

All branches use `username/brief-description`: a lowercase GitHub handle and a
lowercase kebab-case description. Agent branches use the `codex/` prefix.

Examples:

- `nuzzles/export-stack`
- `nuzzles/preserve-codex-settings`
- `codex/export-stack`

### Pull Requests

PR titles follow Conventional Commits: `type: description`, with an optional
scope (`type(scope): description`). Use a lowercase description, such as
`feat: export a stack` or `fix(codex): preserve existing configuration`.
Allowed types are `feat`, `fix`, `docs`, `style`, `refactor`, `perf`, `test`,
`build`, `ci`, `chore`, and `revert`.

CI lints titles and descriptions in [.github/workflows/pr-lint.yml](.github/workflows/pr-lint.yml),
including after edits.

Use the [PR template](.github/PULL_REQUEST_TEMPLATE.md).
Keep each PR focused on one coherent change and link any
relevant issue. Describe the resulting behavior, important design decisions,
exact checks and manual scenarios run, and known limitations or follow-up work.
CI requires the template's sections in order with content in each section;
comments and empty checkboxes do not count. Write `None` for sections that do not
apply. This check also reruns after PR description edits.
Resolve review feedback and rerun affected checks after the final change.
Report the PR link when handing off completed work.

## README policy

Keep README.md very concise. README edits and additions must be made by a human
or be targeted changes explicitly requested by a human. Agents must not expand
or update the README as part of routine implementation or documentation work.

## Checks

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

## Cross-platform diagnostics and discovery

Keep errors in shared code platform-neutral and actionable on macOS, Linux, and
Windows. Do not name a specific OS or its installation paths in a cross-platform
error message; put platform-specific guidance in documentation or a diagnostic
that is compiled only for that platform.

Test Codex discovery on all three platforms. Standard CLI installations (npm,
Homebrew, and native binaries) are discovered through PATH; Windows supports
native `codex.exe` and npm `codex.cmd` launchers. The macOS desktop fallback checks
system and user Applications folders, with `MCPSTACK_CODEX_APP` as an explicit
bundle override. Test PATH order, paths with spaces, missing launchers, and
version-command failures without depending on a real Codex installation.

## Changing the MSRV

Update `package.rust-version` in Cargo.toml, the README's current MSRV statement,
and the current version's row in its compatibility table. Preserve rows for
previous released versions when adding a new version. The dedicated MSRV CI job
reads Cargo.toml automatically and runs `cargo check --locked` with the declared
toolchain.

Unless explicitly stated otherwise, contributions are dual licensed under
MIT OR Apache-2.0, without additional terms or conditions.
