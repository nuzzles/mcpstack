# Changelog

Notable changes to mcpstack are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and version numbers
follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## Unreleased

## [0.2.0] - 2026-10-22

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

Initial release.

[0.2.0]: https://github.com/nuzzles/mcpstack/compare/release-0.1.0...01044ce
[0.1.0]: https://github.com/nuzzles/mcpstack/tree/release-0.1.0
