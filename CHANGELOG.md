# Changelog

<!--
This changelog follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
Changes are grouped under Added, Changed, Deprecated, Removed, Fixed, and Security.
Version numbers follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
-->

The latest published mcpstack release is [0.1.0 on crates.io](https://crates.io/crates/mcpstack/0.1.0),
released on 2026-10-12. Its changes are [documented below](#010---2026-10-12).

## [Unreleased]

This release has an [MSRV][] of 1.98.

## [0.2.0] - 2026-10-22

This release has an [MSRV][] of 1.98.

### Added

- Run `list`, `show`, `export`, `diff`, `import`, and `use` without a harness
  namespace. Interactive commands prompt for the target harness, and for a
  stack source or export file when needed. Installed harnesses appear first.
  `import` can select multiple harnesses; other commands select one. Use
  `--harness` for noninteractive operation. Import asks for harnesses before
  its source and reports a no-op if none are selected.
- Load stacks from HTTP or HTTPS URLs with `validate` and the Codex and Claude
  Code `diff`, `import`, and `use` commands. Remote stacks require absolute or
  client-resolved command and working-directory paths.

## [0.1.0] - 2026-10-12

This release has an [MSRV][] of 1.98.

### Added

- Initial release.

[Unreleased]: https://github.com/nuzzles/mcpstack/compare/01044ce...main
[0.2.0]: https://github.com/nuzzles/mcpstack/compare/release-0.1.0...01044ce
[0.1.0]: https://github.com/nuzzles/mcpstack/tree/release-0.1.0
[MSRV]: README.md#msrv
