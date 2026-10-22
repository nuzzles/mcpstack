# Changelog

Notable changes to mcpstack are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and version numbers
follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Run `list`, `show`, `export`, `diff`, `import`, and `use` without a harness
  namespace. Interactive commands prompt for the target harness, and for a
  stack source or export file when needed. Installed harnesses appear first.
  `import` can select multiple harnesses; other commands select one. Use
  `--harness` for noninteractive operation.
- Load stacks from HTTP or HTTPS URLs with `validate` and the Codex and Claude
  Code `diff`, `import`, and `use` commands. Remote stacks require absolute or
  client-resolved command and working-directory paths.

## [0.1.0]

Initial release.
