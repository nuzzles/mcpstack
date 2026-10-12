# Functional requirements

**Done:** implemented and verified.\
**Partial:** some behavior works; always add a linked asterisk beside the status
to its entry in Current status at the bottom.\
**Unsupported:** not implemented yet.

## Phase 1: MVP

### Harness Support

#### Codex

| Implementation | Brief description |
| --- | --- |
| Done | Read MCP server entries from the default Codex config for export. [*](#codex-export) |
| Done | Read MCP config at an explicit path for export. |
| Done | Write MCP config at the default or an explicit path for import. [*](#codex-import) |
| Done | Validate MCP fields against the bundled current Codex config schema; reject obsolete or unknown fields. [*](#codex-config-schema) |
| Done | On import, preserve unrelated settings/servers; skip identical entries and require approval for replacements. |

### Logging and ANSI

| Implementation | Brief description |
| --- | --- |
| Done | Send compact tracing logs to stderr; keep YAML shown on stdout and CLI schema JSON clean. |
| Done | Support `-v`/`-vv`, `--quiet`, and `--log`/`RUST_LOG` filtering. |
| Done | Support `--color auto/always/never` and `MCPSTACK_COLOR` for help, diagnostics, logs, and diffs; auto checks the destination terminal. |
| Done | Honor `--no-color` and nonempty `NO_COLOR`; enable ANSI support on Windows. |
| Done | Keep configuration values out of logs; retain typed error codes and exit statuses. |

### CLI

| Implementation | Brief description |
| --- | --- |
| Done | Predictable commands, help, and examples. [*](#commands-and-help) |
| Done | Export the entire CLI argument schema for AI consumption. [*](#cli-schema) |
| Done | Stable exit statuses and actionable error codes. [*](#exit-statuses-and-error-codes) |
| Done | Default interactive prompts and explicit noninteractive operation. [*](#interaction) |
| Done | Deterministic stack files and configuration previews. [*](#deterministic-output) |
| Done | Human-readable operation results; diagnostics on stderr. [*](#output-formats) |
| Done | Define a versioned, client-independent stack format. |
| Done | Store permissions in a shared format without harness ownership; preserve supported permission behavior on round trips and reject lossy conversions. [*](#shared-permissions) |
| Done | Validate schema versions, server definitions, names, and secret references. |
| Done | Show Codex server entries as YAML on stdout or export them to a named stack file with safe defaults. [*](#codex-export) |
| Done | Replace recognized credentials with secret references by default; allow interactive selections or `--expose-secrets` to include values. |
| Done | Import/merge a stack into Codex without losing supported fields. [*](#phase-1-boundaries) |
| Done | Resolve masked literals from environment variables or hidden prompts; preserve runtime bindings. [*](#secrets) |
| Done | Offer a unique numbered backup after validation and approval, before applying import; abort if a requested backup fails. [*](#codex-import) |
| Done | Atomic config writes with Unix `0600` permissions or inherited Windows directory permissions; preserve originals on failure. [*](#codex-import) |
| Done | Test round trips, repeat imports, config schema compatibility, missing secrets, and write failures. [*](#stack-workflow-tests) |

### Testing

| Implementation | Brief description |
| --- | --- |
| Done | Run CLI integration tests in CI on macOS, Linux, and Windows. [*](#cross-platform-integration-tests) |

## Phase 2

### Harness Support

#### Claude

| Implementation | Brief description |
| --- | --- |
| Done | Claude Code user-scope JSON list, import, show, export, diff, and stack switching. [*](#claude-support) |

### CLI

| Implementation | Brief description |
| --- | --- |
| Done | Make all CLI output readable and understandable to both humans and AI agents. |
| Done | List server names, transports, commands/endpoints, and available configuration state. [*](#listing-servers) |
| Done | Select servers by name; reject unknown selections. [*](#server-selection) |
| Done | Switch between stack files, replacing the complete MCP server set. [*](#switching-stacks) |
| Done | Produce deterministic YAML exports; require confirmation or `--overwrite` for existing files. [*](#codex-export) |
| Done | Keep discovery, validation, and planning free of file writes and server execution. [*](#codex-import) |
| Done | Preview server additions, updates, and removals; require approval for replacements and report configuration validation errors before applying changes. [*](#codex-import) |
| Done | Show planned configuration writes with secrets redacted. [*](#output-formats) |
| Done | Resolve literal relative executable and working-directory paths from the stack location; identify the target config. [*](#stack-relative-paths) |
| Done | Apply approved file/stack changes as one transaction, the only supported apply mode. [*](#transactions) |
| Done | Require explicit conflict resolution and validate configuration before writes. [*](#codex-import) |
| Done | Create numbered recovery copies using the documented platform permissions, and reject detected changes since the configuration was read. [*](#codex-import) |
| Done | Repeat imports and stack switches without duplicates or unnecessary side effects when the stack, resolved values, and target configuration are unchanged. [*](#codex-import) |
| Done | Mask recognized credentials in shared exports by default, expose them only on explicit request, and redact them from previews and diagnostics. [*](#secrets) |
| Done | Test secret redaction and configuration preservation on success and failure for supported workflows. [*](#stack-workflow-tests) |
| Done | Document human and AI workflows, failure handling, switching, and repeat imports. [*](#documentation-site) |

## Notes

### Version compatibility

[*] Stack files decode into `Stack::V1(StackV1)`; readers and writers dispatch
through this enum. Stack schema versions are independent of Codex configuration fields. Codex
config.toml has no format-version field. Import and export use a bundled snapshot
of the current official MCP config schema without detecting or running Codex.
Reject unsupported stack schemas, obsolete or unknown MCP fields, invalid values,
and unsupported portable transports before writing. Preserve unrelated settings.

### Client-independent server definitions

Stack items have no `client` field. A server contains either `config` with
configuration data or `transport` and optional `settings`. The export/import
command selects the client adapter; a stack does not record client ownership.
For example:

```yaml
schema_version: 1
servers:
  local:
    config:
      command: example
      args: [--token, {$env: SERVICE_TOKEN}]
```

Configuration data preserves fields and nested secret references without binding
an item to its source client. Each target adapter must validate that it can
represent the fields before writing; removing the label does not make every
client-specific option universally supported. Codex and Claude Code have
adapters. Transport definitions provide the explicit shared transport form.

To migrate legacy stacks, remove `client: codex` (or another client label) from
existing items and retain their `config` data. Legacy client labels
are rejected rather than silently ignored.

### Phase 1 boundaries

[*] Import merges server definitions into Codex config without installing or
starting server software. Validate and resolve the stack before changing config.
Preserve unrelated settings, skip identical servers, and apply only approved
additions or replacements. Backups and file writes are described under
[Codex import](#codex-import).
`diff` previews detailed redacted configuration changes. `import` and `use`,
including their dry runs, show named server change summaries before any approval
prompts. Configuration-only stack switching is implemented by `use`.

---

### CLI schema

[*] `--schema` emits a versioned JSON description of every implemented command,
argument, option, type, default, requirement, constraint, and example. An AI can
read it without scraping help text. Generate it from the command definitions to
avoid drift. Schema output is independent of interactive controls.

### Interaction

[*] Prompt when stdin and stderr are terminals. `--non-interactive` or redirected
input disables prompts. Export defaults to masking credentials; `--expose-secrets`
includes them without prompting.

Import defaults to skipping each addition or replacement. Both clients offer
per-server approval; Codex also offers bulk choices. A named change summary
appears before approval prompts. Added, modified, and deleted entries use green
`+`, yellow `~`, and red `-` when color is enabled; plain output keeps the markers.
`-y` / `--auto-approve` approves all changes. `import --dry-run` retains server
approval prompts; use `--non-interactive -y` for unattended import previews.
`-y` alone skips approval prompts but can still prompt for missing secrets during
previews. With `--non-interactive`, missing secrets fail without prompting.
`use --dry-run` has no approval prompt and does not require `-y`.

`diff` shows detailed redacted changes without server approval prompts. `diff`,
`import --dry-run`, and `use --dry-run` can ask for missing masked values when
interactive and never write files. Before requesting missing secrets, `diff`
shows a preliminary redacted diff, while import and use show a preliminary
change summary. The exact plan is recomputed after values are supplied and
before any approval prompt. Actual imports and switches with `-y` require
masked values to be available in the environment.

### Claude support

[*] Claude Code user-scope support is complete. `mcpstack claude` reads and writes
user-scope `~/.claude.json` (or `--config <path>`) and preserves unrelated JSON
settings. It supports native stdio, HTTP, and SSE server definitions plus portable
stdio/HTTP/SSE definitions representable in Claude Code. Unsupported fields and
transports fail before writes. Diff and export mask recognized credentials; import
and use create numbered backups of changed existing files before transactional
file replacement. Supported server fields are `type`, `command`, `args`, `env`,
`url`, and `headers`.
Claude Desktop and project/local scopes are outside this user-scope integration.
For unsupported stack fields, Claude Code operations report every affected
server and field name without printing field values.
Codex export skips `computer-use` and `node_repl` by default. Claude Code export
skips its built-in names `workspace`, `claude-in-chrome`, `computer-use`,
`Claude Preview`, and `Claude Browser` by default. Each client exposes only its
relevant `--include-*` flags on `list`, `show`, and `export`; `--all` includes every normally
skipped name. Claude Code `show` and `export` accept `--permissions-config`;
`import`, `use`, and `diff` accept it with `--include-permissions`.
The filter runs before server validation and leaves source configuration unchanged.
Codex `http_headers` map to portable `headers`. `bearer_token_env_var` maps to
`headers.Authorization: {env: VAR, prefix: "Bearer "}`; `env_http_headers` maps
to `{env: VAR}` header references. Claude Code emits these as `${VAR}` strings,
preserving runtime lookup. Claude Code may suppress
expansion of its protected credential variables, so those names require a separate
user-managed environment variable. Duplicate header names fail instead of
overwriting either value. Claude Code's reserved server names also fail before
writes. Claude Code cannot apply per-server Codex startup timeouts.
Both exporters emit portable `transport` entries. Permission export is opt-in
only with `--include-permissions`; `--all` changes server selection only.
Both adapters convert native policies to the same
[shared permission format](#shared-permissions). Import, use, and diff ignore
permission metadata and tool allow/deny filters unless passed `--include-permissions`.
Without that flag, existing target permissions and filters are preserved, including
when using older stacks with filters in `settings` or native `config` entries.
Conversions that cannot preserve the policy fail before backups or writes.
Claude settings are read and written in
`settings.json` (or the file selected by `--permissions-config`), with backups
for changed settings. Project, managed, and rules outside the chosen settings
file are outside this export scope. Other unmappable source fields fail export
without emitting a partial stack.
Claude Code permission rules live in `settings.json` under `permissions.allow`,
`permissions.ask`, and `permissions.deny`; a rule such as `mcp__obsidian__*`
matches tools from that server. These rules are outside MCP server transport data.
Older v1 stacks with `bearer_token` are accepted and normalized to an
Authorization header when read. New exports write only `headers`.

### Shared permissions

Permission metadata belongs to the server, with no `source` or harness tag.
Rules use local tool names rather than native `mcp__server__tool` spellings:

```yaml
permissions:
  allow: [read]
  ask: [write]
  deny: [delete]
```

`allow` approves matching tools, `ask` requires approval, and `deny` blocks them.
`auto` and `writes` preserve automatic and write-sensitive approval decisions
for exact tool names.
Optional `default_mode: allow|ask|auto|writes` is a fallback for tools without an
explicit approval decision. The `allow`/`ask`/`deny` lists can contain `'*'` for
all tools or tool patterns;
within matching rules, deny takes precedence over ask, then allow. A wildcard
ask rule therefore also constrains tools with an explicit allow rule, unlike an
ask fallback. Empty `permissions: {}` explicitly clears captured permission
decisions when included; omitted metadata leaves existing approval policy alone.

Codex translates `allow`/`ask` to `approve`/`prompt`, retains `auto`/`writes`, and
stores exact denials as `disabled_tools`. Its exporter moves those denials into
permission metadata when requested. Enabled-tool filters remain in `settings`,
but are exported and applied only with the same explicit opt-in. Claude adds native
server prefixes only when writing its `allow`, `ask`, and `deny` arrays. Semantically equivalent native
spellings, such as a bare server rule and its all-tools wildcard, can normalize.
Lossless restoration concerns permission decisions, not JSON/TOML layout or spelling.

Both native models can be exported and restored using this shared representation.
The target must support every decision being restored: Claude rejects `auto`,
`writes`, and default-ask policies with allow overrides; Codex rejects partial
tool patterns, wildcard denials, and overlapping approval decisions it cannot
retain. Claude rejects permission export and application when server names overlap
at a `__` separator, such as `a` and `a__b`, including existing or unselected
servers. Rename one server before handling those permissions.
No unsupported decision is silently discarded or weakened. Existing
transport/settings compatibility checks still apply.

Legacy `source: claude` and `source: codex` policies, and native-qualified
permission rules, are rejected.
Re-export native configurations with `--include-permissions` to migrate them.

### Switching stacks

[*] `mcpstack codex use <file>` selects a stack file and replaces
all MCP servers as the selected stack's complete desired set. Servers absent from
the file are removed, including servers added outside mcpstack; unrelated client
settings are preserved. Resolve the new stack's secrets locally.

`codex use` selects the client explicitly for both writes and dry runs. Use
`--config <path>` to override its default config path. It validates the entire
stack and resolves secrets before any backup or write. Interactive use shows a
named change summary, asks once to replace ALL servers (default No), then reports
the result. `-y` approves the whole switch; unattended writes require it.
`--dry-run` summarizes additions, modifications, and removals without approval
prompts, backups, or writes. Missing masked values still fail in unattended mode.
Changed switches always create a numbered
backup, then atomically replace the config with stale-snapshot checks. Backup or
write failures abort; identical sets do not rewrite files or create backups. An
empty stack clears every server. Switching changes configuration only; it does
not install or start server software. Stack names/aliases and additional client
adapters remain future work.

### Listing servers

`mcpstack codex list` and `mcpstack claude list` read the same default configuration
paths as their adapters, or the file selected by `--config <path>`. They print a
table sorted by server name with transport, configuration state, and command or
endpoint. Listing shares the same default exclusions and client-specific
`--include-*` flags as show/export; `--all` includes every normally skipped name.
The shared filter runs before server validation. It does not prompt,
resolve secrets, write files or backups, run helpers, or contact MCP servers.

Codex state is `enabled` (the default when absent) or `disabled`. Claude lists
user-scope definitions as `configured`, and its reserved names as `ignored`.
Output contains only the table, without an explanatory footer.
Claude project and permission overrides are not evaluated; configuration state
does not establish connection health. No synthetic per-server enabled flag is
invented for Claude.

Only the executable is shown for stdio, without arguments or environment values.
HTTP/SSE endpoints omit URL user info, queries, and fragments; headers are never
displayed. Names and commands escape terminal control characters. No new
credential values are read from the environment. Listing validates its displayed
fields without rejecting unrelated or newer settings. Invalid definitions fail
with `CONFIG_READ_ERROR` and no partial table. A missing configuration fails;
an empty configuration or server map reports `No MCP servers configured.` If filters
hide every configured server, output explains that no servers matched and suggests
`--all` to include skipped servers.

### Server selection

Both clients accept repeatable `--server <NAME>` on `list`, `show`, `export`,
`import`, and `diff`. Names match exactly and case-sensitively; repeating a name
does not duplicate it. With no selection, commands retain their existing behavior.
For example: `mcpstack codex export work.yml --server github --server linear` or
`mcpstack claude import work.yml --server github`.

List/show/export select from the source config before validating server fields
or prompting for credentials. Default exclusions still apply: explicitly selecting
a skipped name requires its `--include-*` flag or `--all`. `--all` enables skipped
names but does not expand an explicit selection.

Import/diff select from the stack before adapter conversion, secret resolution,
and permission planning. Unselected target servers and their permission rules
remain unchanged. The entire stack must still satisfy the stack schema.
Unknown names or explicitly selected excluded names fail with `INVALID_ARGUMENT`
(exit 2) and no partial output, exports, or backups. `use` does not accept
`--server`; it continues to replace the whole server set.

### Stack-relative paths

Both clients' `import`, `use`, and `diff` resolve literal relative `command`
paths and `cwd` values from the directory containing the input stack. Portable
stdio transports and native configuration definitions share this behavior.
For `stacks/work.yml`, `command: ./bin/server` points to `stacks/bin/server` and
`cwd: workspace` points to `stacks/workspace`. Each field is relative to the stack
directory; the command is not resolved relative to `cwd`.

Bare executable names such as `npx`, `uvx`, and `docker` remain PATH lookups.
In stack-relative mode, absolute/rooted paths remain unchanged. Relative executable paths must contain
a platform path separator (or be `.`/`..`); use `/` in stacks shared between
platforms. Windows also accepts `\\` separators and rejects drive-relative paths
such as `C:server.exe`. Existing Windows drive-rooted and UNC paths are preserved.

Only literal command/cwd fields are resolved. Arguments, environment values,
URLs, secret references, and strings containing client-side `${...}` expansion
are unchanged. There is no shell expansion or executable/prerequisite check.
Claude still rejects `cwd` because its server definitions cannot represent it.

The stack's containing directory is resolved to an absolute directory. A symlinked
stack uses the directory containing the link, not the linked file's directory.
Server paths need not exist and are not canonicalized; previews and writes use
the same resolved values. Loading does not modify the source stack.

Stdio transports accept `path_resolution: stack` (the default) or
`path_resolution: client`. Both clients' show/export commands emit `client` to
preserve the original command and cwd literally: relative paths retain the
client's runtime semantics instead of being rebased to the exported stack's
directory. This preserves export/import round trips without guessing a launch
directory. The marker is stack metadata and is never written into client configs.
Use the default `stack` behavior for paths bundled alongside an authored stack.

The stack filename and `--config` argument themselves remain relative to the CLI's
working directory. Change summaries show the absolute target config path directly
above the added/modified/deleted summary and before approval prompts. Control
characters in the displayed path are escaped so filenames cannot alter the terminal.

### Transactions

[*] Whole-file/stack transactions are the only supported apply mode. Validate and
prepare all approved configuration changes before committing them.
Codex uses one atomic config replacement and now creates automatic import backups
only after validation and approval; invalid inputs and no-op imports create no
backups. Claude serializes both server config and permission settings before
writing, checks all snapshots, and creates numbered backups of changed existing
files before replacing either file. Unchanged files are checked without being
rewritten, and duplicate target paths are rejected.

If a Claude write fails, earlier writes are restored in reverse order, including
removing files that did not originally exist. Recovery checks for the exact bytes
written by this operation and refuses to overwrite detected concurrent edits.
Recovery continues for other files if one cannot be restored, then reports an
incomplete recovery and directs the user to inspect the files and backups.
Successful backups remain available after failures.

This is a small in-process file transaction, not an atomic multi-file commit or
crash/interruption recovery. Snapshot checks still have a final check/rename (or
check/remove) race; no locking or journal is added.

### Secrets

[*] Masked literal fields use `{"$env":"VARIABLE"}` in configuration definitions or
`{value_env: VARIABLE}` in portable secret fields. Resolve them from the shell or hidden
interactive input, caching repeated references. Missing values fail when prompting
is unavailable, including actual imports and switches with `-y`. `diff`,
`import --dry-run`, and `use --dry-run` can prompt for missing values when
interactive and always compare real values.

Native `env_vars`, `bearer_token_env_var`, and `env_http_headers` retain their runtime
bindings without embedding values. Missing local runtime variables warn; remote
bindings are not checked against the local shell. Masking does not convert literal
fields into runtime bindings. `{env: VARIABLE}` remains a runtime binding in portable
headers; `{value_env: VARIABLE}` restores a masked literal during import. Export
recognition and exposure are described under
[Codex export](#codex-export).
Credential recognition uses field and argument names; it does not identify every
secret embedded in an arbitrary URL or unnamed argument. Imported client configs
and recovery backups can contain literal credentials, including server arguments.

## Open decisions

A standalone `doctor` command and general configuration/environment diagnostics
are outside scope. Commands still validate their inputs and report actionable
failures.

Additional command/flag syntax, client adapter support, and backup retention/recovery.
Stack format v1 defines YAML, STDIO/HTTP/SSE/WebSocket, client-independent
configuration and transport definitions, and environment reference syntax.

Remote catalogs, team access controls, and automatic synchronization are later
scope. CLI publishing and release/deployment automation require explicit authorization.
Documentation publishing to GitHub Pages is supported.

## Current status

### Documentation site

The mdBook site at `https://nuzzles.github.io/mcpstack/` is built from
[`docs/src`](docs/src/SUMMARY.md) and documents mcpstack 0.1.0. It covers source installation,
scratch-config examples, export/import/switch workflows, secrets, permission
opt-in, compatibility, failures, recovery, and repeat operations. Each chapter
also has a Markdown download. `llms.txt` indexes the agent-readable pages and
`llms-full.txt` combines them. The CLI reference and `cli-schema.json` are generated
from the executable during the same build. The generated reference and agent
indexes identify the executable's version.

PRs build and validate the site, YAML examples, scratch workflows, local links,
and anchors. Merged changes deploy through GitHub Pages; PRs cannot deploy.
Contributor build instructions are in [`docs/README.md`](docs/README.md).

### Commands and help

Client operations are grouped under `mcpstack <client> <operation>`. Codex exposes
`list`, `use`, `import`, `show`, `export`, and `diff`; `validate` and `--schema` stay at the top
level. Claude Code exposes the same six operations under `claude`. The parent
command selects the target, so there is no `--client` option.
The previous operation-first syntax is no longer accepted.

Help, version, `--schema`, `validate <file>`, `codex list`, `claude list`, `codex show`, `codex export <file>`,
`codex diff <file>`, `codex import <file>`, `codex use <file>`, and runnable examples are implemented.
CLI integration tests verify help/version output, the generated argument schema,
runnable examples, and rejection of invalid arguments.

Usage errors use Clap's detailed diagnostics, including missing arguments,
subcommands, conflicting options, valid choices, and spelling suggestions.
When color is enabled, errors are red, invalid names are yellow, and options
and suggestions are cyan. Diagnostics go
to stderr with exit 2 and no error-code footer; help and version go
to stdout with exit 0. Command and option names remain visible. Supplied
values, unexpected positional arguments, and inline assignment values are
redacted; help names environment variables without printing their contents.

### Exit statuses and error codes

CLI exit statuses are 0 for success/help/version, 1 for output failures
(`OUTPUT_ERROR`), 2 for invalid arguments (`INVALID_ARGUMENT`), 3 for stack read
failures (`STACK_READ_ERROR`), and 4 for invalid stacks (`INVALID_STACK`).
Config read failures use 5 (`CONFIG_READ_ERROR`); export failures use 6
(`EXPORT_ERROR`). Logging initialization/filter failures use exit 9
(`LOGGING_ERROR`). Removed statuses 7, 8, and 12 are not reused.
Import failures use 10 (`IMPORT_ERROR`), backup failures use 11 (`BACKUP_ERROR`),
config merge/write failures use 13 (`CONFIG_WRITE_ERROR`), and exported stack
file write failures use 14 (`STACK_WRITE_ERROR`).
The CLI schema advertises these statuses and actionable descriptions from the
typed error definitions. Tests check code/status uniqueness and process failures
for invalid arguments, output failures, invalid or unreadable stacks, config read
failures, export failures, and import failures.

### Deterministic output

The CLI schema is generated deterministically from command definitions. Stack v1
uses ordered maps and passes serialization round-trip tests. Codex show and export emit
deterministic client-independent YAML stacks. `diff` emits deterministic redacted
configuration previews for the same stack, target config, resolved secrets, and
color settings. Use and import dry-runs summarize server changes. Server
processing uses ordered maps; diff previews retain the target file's layout
and line numbers.
Claude writes replace only the top-level `mcpServers` value in the JSON text,
preserving unrelated settings and their formatting. Previews compute hunk
positions from that exact proposed write. Preview placeholders redact credentials
and unrelated settings, so the displayed patch is not independently applicable.

### Output formats

Help, version, server inventories, and validation results use text output;
`--schema` emits JSON; both clients' `show` commands emit YAML on stdout, while
`export <file>` writes the same YAML to the named file. `diff` emits a redacted
unified diff; import and use dry-runs summarize added, modified, and deleted servers.
Diagnostics go to stderr without echoing argument values. Automation uses the
noninteractive commands and stable exit statuses; structured JSON operation
results are not required.

Detailed server configuration diffs are implemented for both clients.
`claude diff --include-permissions` also shows the planned `allow`, `ask`, and
`deny` edits in a separate settings-file diff, including permission-only changes.
It uses the same serialization as the settings write, preserving original and
proposed file line numbers. Unrelated settings and non-MCP rules are omitted
from the preview. No-op settings produce no diff; previews create no files or
backups. With `--include-permissions`, import and use show the target settings
path and added, modified, and deleted MCP rules before approval, including in
dry runs. Modified rules show their old and new `allow`/`ask`/`deny` memberships.
Import refreshes the permission summary when server approvals narrow the plan.

### Human and agent readable output

All output, including help, prompts, previews, results, diagnostics, and logs,
must be understandable to both humans and AI agents. Use clear, consistent labels
and enough context to identify targets, actions, outcomes, and next steps when
needed. Preserve meaning in plain text without relying on color or terminal UI.

Both clients identify targets and label added, modified, and deleted items in
plain text. No-op diff/import/use results explicitly report that no changes are
needed. Successful dry runs state that no configuration files were changed;
imports report the changed server count and Claude permission-only imports report
settings changes. Plans, completed writes, declined changes, and cancellations
have distinct messages. Import prompts identify additions versus replacements
and show progress through servers requiring approval.

Help describes summary previews, approval behavior, permission opt-in, and examples
for the selected client. File paths in results are absolute and escape control
characters. YAML exports and JSON CLI schema remain clean on stdout; diagnostics
and logs use stderr. Fatal diagnostics retain their stable error code and failure
exit status even with `--log off` or filtering through `RUST_LOG`.

Integration tests cover these results, channel separation, color controls, and
secret redaction. Terminal checks also verify declined imports, stack switches,
and permission approval leave configuration unchanged and create no backups.

### Cross-platform integration tests

The Phase 1 CLI must have integration tests that execute the compiled binary on
macOS, Linux, and Windows. CI runs `tests/cli.rs` through `cargo test --locked` on
`macos-latest`, `ubuntu-latest`, and `windows-latest`. Keep this matrix as workflows
are added; cross-compilation and unit tests alone do not satisfy this requirement.
Workflow coverage and remaining limitations are tracked under
[Stack workflow tests](#stack-workflow-tests).

### Stack workflow tests

Stack serialization round trips, client-independent documents, reference syntax,
unsupported shared fields, configuration fields and secret references, transport
URL schemes, validation file preservation, and CLI output failures are
covered. Current Codex schema validation, diff redaction and file line numbers, and
export without a Codex installation are also covered. Repeat imports, secret
resolution, backups, atomic writes, and failure preservation are covered on Unix
and Windows. Windows tests also verify case-insensitive backup names and locked-file
replacement failures. Long paths cover config creation, custom/numbered backups,
replacement, and repeat imports.
Claude permission preview tests cover rule additions, removals and moves,
redaction in formatted/minified settings, file line numbers against actual writes,
missing settings files, no-op results, server selection, Codex rule translation,
color controls, permission summaries before approval, normalization versus no-op
changes, and malformed inputs without writes.
Shared permission tests cover all Codex approval modes, Claude patterns and
overlapping rules, native and cross-client round trips, explicit empty policies,
strict opt-in (including `--all` and legacy tool filters), ambiguous Claude server
names, legacy-format rejection, and failures without partial writes.
Other platforms reject import safely.

### Codex export

`codex show` reads the default Codex TOML config and prints client-independent server
entries as portable YAML `transport` definitions on stdout. `codex export <file>`
writes the same YAML to a required `.yml` or `.yaml` destination path. In noninteractive use,
credential values become `{value_env: MCPSTACK_SERVER_FIELD}` references. Recognition uses credential field names
(token, secret, password, API/access/private key), authorization/cookie headers,
and named token arguments (`--token VALUE` or `--token=VALUE`). Nested credential
fields and matching environment variable names are covered. Commands, URLs,
ordinary arguments and existing environment-name settings retain their meaning.
This is name-based detection, not a guarantee that arbitrary
unnamed values or credentials embedded in URLs will be recognized.

Nested fields and array indices contribute to reference names. Names are uppercase
ASCII with punctuation replaced by underscores; collisions receive deterministic
numeric suffixes. Existing references are preserved and their names reserved.
Export never reads or sets environment variables. For an inline credential argument,
the referenced environment value must contain the complete `--token=VALUE` argument.

In an interactive terminal, `codex show` and `codex export` ask whether to mask or include each
detected credential, showing progress such as `Secret 1/12`. Masking is selected
by default; Yes/No to all applies the chosen action to the current and remaining
credentials. `--non-interactive` masks all detected credentials without prompting,
including when standard input is redirected. `--expose-secrets` on either command
preserves literal values, including credentials, without prompting.
No export mode changes the source file or logs its values. Server names and field
keys remain visible. `--config <path>` reads an explicit TOML file instead of the default
config. Relative paths resolve from the current working directory. Both clients'
file exports create new destinations without confirmation. An existing destination,
including an empty file, requires an overwrite prompt (default No) or `--overwrite`.
`--non-interactive` and redirected input reject existing files unless `--overwrite`
is supplied. Declining cancels successfully without changing the file. The overwrite
check runs before credential prompts; `--overwrite` does not change secret masking.
Writes remain atomic, and a destination created or changed after the initial
snapshot is rejected. Invalid destinations and attempts to replace the explicit
source config are rejected even with `--overwrite`.

### Codex config schema

Import conversion and export validate MCP entries against
`src/integrations/codex/mcp.schema.json`, extracted from the current official Codex config
schema. Validation is offline and does not require Codex to be installed. Native
config definitions preserve supported fields on import; exported portable
definitions preserve supported transport and settings. Obsolete inline `bearer_token` and unknown
fields are rejected without exposing values. Refresh the snapshot deliberately
when adding support for schema changes. Portable conversion supports STDIO/HTTP;
filesystem writes support Unix and Windows.

Export omits `sandbox_mode` and `approval_policy` when they appear directly in an
MCP server definition. These policy fields are not part of the exported server
stack; Codex import/use preserve their existing values for servers with the same
name. Permission fields also remain in the target unless `--include-permissions`
requests restoration. Other unsupported server fields still fail validation.

### Codex import

All Codex commands accept `--config <path>`; otherwise they use
`$CODEX_HOME/config.toml` or `~/.codex/config.toml`. Paths resolve from the current
directory. Import and diff validate the stack against the bundled MCP schema
without running Codex or servers.

| Command | Behavior |
| --- | --- |
| `codex diff stack.yml` | Show all proposed changes; prompt only for missing masked values. |
| `codex import stack.yml` | Summarize proposed additions/replacements, approve servers, offer backup, then apply. |
| `codex import stack.yml -y` | Approve all changes automatically; after validation, create a default backup and apply if changes are needed. |
| `codex import stack.yml --dry-run` | Show proposed changes before server prompts, then report the approved count without writes. |

Backups default to numbered sibling files (`config.toml.~1~`, `config.toml.~2~`,
etc.) beyond the highest existing generation. The prompt allows another path or
skipping backup. Both automatically approved and interactive imports create
requested backups after stack validation, change preparation, and server approval.
Invalid stacks, missing secrets, dry runs, and no-op imports create no backups.
Backups never overwrite existing files and remain after later failures.

Replacements update the complete server definition. Unrelated settings, comments,
and servers are preserved; identical entries do not rewrite the config. Shared
`src/io/` handles byte snapshots, backups, and file replacement for integrations;
Codex retains TOML parsing and merge policy. The CLI uses a Tokio runtime and
`tokio::fs`, with file creation and atomic persistence on the blocking pool.
Filesystem tests use client-independent byte fixtures. Writes use a synced temporary
file in the same directory and check the original snapshot before atomic replacement.
Unix files are created with mode `0600`. Windows backups and temporary configs inherit
permissions from their destination directory; mcpstack does not enforce user-only
access. Use directories with suitable permissions for configs and backups containing
credentials. Replacement uses the temporary file's inherited permissions rather than
preserving the target's previous ACL. Windows paths are normalized for long-path
support; locked or read-only targets fail without replacing the original. Windows
device paths, alternate data streams, and config reparse points are rejected. Backup
write failures may leave a partial backup. Previews work on all platforms. Concurrent
editing still has a final check/rename race.

Diffs use the original and proposed file line numbers. Recognized credentials and
resolved secrets are redacted; hidden changes display `<redacted: changed>`.
Comments and unrelated values are omitted from preview context. These previews
are for review, not patch application. Successful dry runs report on stdout that
configuration files were not changed and create no files or backups. Import does
not remove servers; `use` replaces the whole server set.
