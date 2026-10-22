# Changelog

Notable changes to mcpstack are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and version numbers
follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Load stacks from HTTP or HTTPS URLs with `validate` and the Codex and Claude
  Code `diff`, `import`, and `use` commands. Remote stacks require absolute or
  client-resolved command and working-directory paths.

## [1.0.0] - Planned

This section outlines the planned stable baseline. Version 1.0.0 has not been
released; the package remains at 0.1.0.

### Baseline

- Manage Codex and Claude Code MCP servers with `list`, `show`, `export`, `diff`,
  `import`, and `use` using the versioned YAML stack format.
- Preview changes, select servers by name, and preserve unrelated client
  settings during imports and stack switches.
- Mask recognized credentials on export and support explicit secret references,
  with permission policies included only when requested.
- Back up changed configuration files and replace each file atomically.
- Provide a machine-readable CLI schema, noninteractive operation, update
  checks, documentation, and an installable agent skill.
