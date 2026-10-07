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
| Partial [*](#codex-import) | Write MCP config at the default or an explicit path for import. |
| Done | Validate MCP fields against the bundled current Codex config schema; reject obsolete or unknown fields. [*](#codex-config-schema) |
| Done | On import, preserve unrelated settings/servers; skip identical entries and require approval for replacements. |

### Logging and ANSI

| Implementation | Brief description |
| --- | --- |
| Done | Send compact tracing logs to stderr; keep YAML exports and CLI schema JSON clean on stdout. |
| Done | Support `-v`/`-vv`, `--quiet`, and `--log`/`RUST_LOG` filtering. |
| Done | Support `--color auto/always/never` and `MCPSTACK_COLOR`; auto checks stderr for logs and stdout for diffs. |
| Done | Honor `--no-color` and nonempty `NO_COLOR`; enable ANSI support on Windows. |
| Done | Keep configuration values out of logs; retain typed error codes and exit statuses. |

### CLI

| Implementation | Brief description |
| --- | --- |
| Partial [*](#commands-and-help) | Predictable commands, help, and examples. |
| Done | Export the entire CLI argument schema for AI consumption. [*](#cli-schema) |
| Partial [*](#exit-statuses-and-error-codes) | Stable exit statuses and actionable error codes. |
| Partial [*](#interaction) | Default interactive prompts and explicit noninteractive operation. |
| Partial [*](#deterministic-output) | Deterministic stack files and plans. |
| Done | Human-readable operation results; diagnostics on stderr. [*](#output-formats) |
| Done | Define a versioned, client-independent stack format. |
| Done | Validate schema versions, server definitions, names, and secret references. |
| Done | Export Codex server entries as a YAML stack to stdout with safe defaults. [*](#codex-export) |
| Done | Replace recognized credentials with secret references by default; allow interactive selections or `--expose-secrets` to include values. |
| Partial [*](#codex-import) | Import/merge a stack into Codex without losing supported fields. [*](#phase-1-boundaries) |
| Done | Resolve masked literals from environment variables or hidden prompts; preserve native runtime bindings. [*](#secrets) |
| Partial [*](#codex-import) | Offer a unique numbered backup before processing import; abort if a requested backup fails. |
| Partial [*](#codex-import) | Atomic config writes with restrictive permissions; preserve originals on failure. |
| Partial [*](#stack-workflow-tests) | Test round trips, repeat imports, config schema compatibility, missing secrets, and write failures. |

### Testing

| Implementation | Brief description |
| --- | --- |
| Done | Run CLI integration tests in CI on macOS, Linux, and Windows. [*](#cross-platform-integration-tests) |

## Phase 2

### Harness Support

#### Claude

| Implementation | Brief description |
| --- | --- |
| Unsupported | Claude support across the full workflow. [*](#claude-support) |

### CLI

| Implementation | Brief description |
| --- | --- |
| Unsupported | List server names, transports, commands/endpoints, and enabled state. |
| Unsupported | Select servers by name; reject unknown selections. |
| Unsupported | Diagnose missing, empty, malformed, and unsupported configs. |
| Unsupported | Save named stacks and switch between them easily. [*](#switching-stacks) |
| Unsupported | Detect machine-specific paths and missing local prerequisites. |
| Unsupported | Produce Git-friendly exports; require explicit overwrite of existing files. |
| Unsupported | Preserve version pins and identify unpinned servers. |
| Partial [*](#codex-import) | Keep discovery, validation, and planning free of writes and server execution. |
| Partial [*](#codex-import) | Preview additions, updates, removals, conflicts, and blockers. |
| Partial [*](#codex-import) | Show planned writes, downloads, and commands with secrets redacted. |
| Unsupported | Resolve relative paths from the stack location; identify the target config. |
| Unsupported | Apply a whole stack as one transaction. [*](#transactions) |
| Unsupported | Optionally continue installing independent servers after failures. [*](#continue-on-failure) |
| Partial [*](#codex-import) | Require explicit conflict resolution and validate prerequisites before writes. |
| Partial [*](#codex-import) | Private recovery copies and stale-plan detection. |
| Partial [*](#codex-import) | Repeat installs without duplicates or unnecessary side effects. |
| Unsupported | Define supported installation mechanisms; reject unsupported ones before writes. |
| Unsupported | Verify servers through MCP connection and initialization. |
| Unsupported | Bound verification time; report per-server and overall results. |
| Unsupported | Diagnose program, credential, authentication, transport, and protocol failures. |
| Unsupported | Clean up verification processes/connections without invoking application tools. |
| Unsupported | Add secret resolution providers beyond environment variables. [*](#secrets) |
| Partial [*](#secrets) | Keep secrets out of output, shared files, CLI arguments, and child-process logs. |
| Partial [*](#stack-workflow-tests) | Test secret redaction and configuration preservation on success and failure. |
| Unsupported | Test sharing a stack between users with different credentials end to end. |
| Unsupported | Document human and AI workflows, failure handling, switching, and repeat installs. |

## Notes

### Version compatibility

[*] Stack files decode into `Stack::V1(StackV1)`; readers and writers dispatch
through this enum. Stack schema versions are independent of Codex configuration fields. Codex
config.toml has no format-version field. Import and export use a bundled snapshot
of the current official MCP config schema without detecting or running Codex.
Reject unsupported stack schemas, obsolete or unknown MCP fields, invalid values,
and unsupported portable transports before writing. Preserve unrelated settings.

### Phase 1 boundaries

[*] Import merges server definitions into Codex config without installing or
starting server software. Validate and resolve the stack before changing config.
Preserve unrelated settings, skip identical servers, and apply only approved
additions or replacements. Backups and private writes are described under
[Codex import](#codex-import).

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

Import defaults to skipping each addition or replacement, with per-server and
bulk choices. `-y` / `--auto-approve` approves all changes. Dry-run retains these
prompts and shows only approved changes; unattended dry-run requires `-y`.
`diff` shows all proposed changes without server approval prompts. Both preview
modes can ask for missing masked values and never write files.

### Claude support

[*] Choose Claude Code, Claude Desktop, or both before implementation. Each
supported client must preserve unrelated configuration.

### Switching stacks

[*] Select a named stack, preview changes, and apply it in one workflow. Track
servers managed by mcpstack so switching can remove obsolete managed entries
without removing unrelated servers. Resolve the new stack's secrets locally.

### Transactions

[*] Validate and prepare the whole stack before committing configuration changes.
On failure, preserve or restore the previous managed configuration. Downloads and
external installer effects may not be reversible; report those explicitly rather
than claiming complete rollback.

### Continue on failure

[*] Provide an explicit alternative to transactional installation: continue with
independent servers, skip dependents of failed servers, and report installed,
failed, and skipped results. Return a nonzero exit status if any server fails.
Transactional mode must not silently commit a partially installed stack.

### Secrets

[*] Masked literal fields use `{"$env":"VARIABLE"}` in native definitions or
`{env: VARIABLE}` in portable secret fields. Resolve them from the shell or hidden
interactive input, caching repeated references. Missing values fail when prompting
is unavailable, including actual imports with `-y`. Both preview modes can prompt
for missing values and always compare real values.

Native `env_vars`, `bearer_token_env_var`, and `env_http_headers` retain their runtime
bindings without embedding values. Missing local runtime variables warn; remote
bindings are not checked against the local shell. Masking does not convert native
fields into runtime bindings. Export recognition and exposure are described under
[Codex export](#codex-export). Additional secret providers remain Phase 2 work.

## Open decisions

Additional command/flag syntax, client adapter support, additional
secret providers, installation mechanisms, and backup retention/recovery.
Stack format v1 defines YAML, STDIO/HTTP/SSE/WebSocket, native client definitions,
environment reference syntax,
and client-independent server definitions.

Remote catalogs, team access controls, and automatic synchronization are later
scope. Publishing and release/deployment automation require explicit authorization.

## Current status

### Commands and help

Help, version, `--schema`, `validate <file>`, `export codex`,
`diff codex <file>`, `import codex <file>`, and runnable help examples are implemented.

### Exit statuses and error codes

CLI exit statuses are 0 for success/help/version, 1 for output failures
(`OUTPUT_ERROR`), 2 for invalid arguments (`INVALID_ARGUMENT`), 3 for stack read
failures (`STACK_READ_ERROR`), and 4 for invalid stacks (`INVALID_STACK`).
Config read failures use 5 (`CONFIG_READ_ERROR`); export failures use 6
(`EXPORT_ERROR`). Logging initialization/filter failures use exit 9
(`LOGGING_ERROR`). Removed statuses 7, 8, and 12 are not reused.
Import failures use 10 (`IMPORT_ERROR`), backup failures use 11 (`BACKUP_ERROR`),
and config merge/write failures use 13 (`CONFIG_WRITE_ERROR`).

### Deterministic output

The CLI schema is generated deterministically from command definitions. Stack v1
uses ordered maps and passes serialization round-trip tests. Codex export emits
deterministic native YAML stacks. Plans are pending.

### Output formats

Help, version, and validation results use text output;
`--schema` emits JSON; `export codex` emits YAML; `diff` and import dry-run
emit redacted unified diffs.
Diagnostics go to stderr without echoing argument values. Automation uses the
noninteractive commands and stable exit statuses; structured JSON operation
results are not required.

### Cross-platform integration tests

The Phase 1 CLI must have integration tests that execute the compiled binary on
macOS, Linux, and Windows. CI runs `tests/cli.rs` through `cargo test --locked` on
`macos-latest`, `ubuntu-latest`, and `windows-latest`. Keep this matrix as workflows
are added; cross-compilation and unit tests alone do not satisfy this requirement.
Workflow coverage and remaining limitations are tracked under
[Stack workflow tests](#stack-workflow-tests).

### Stack workflow tests

Stack serialization round trips, client-independent documents, reference syntax,
unsupported shared fields, native client fields and secret references, transport
URL schemes, validation file preservation, and CLI output failures are
covered. Current Codex schema validation, diff redaction and file line numbers, and
export without a Codex installation are also covered. Repeat imports, secret resolution, backups, atomic writes, and failure
preservation are covered on Unix; other platforms reject import safely.

### Codex export

`export codex` reads the default Codex TOML config and prints native server
entries as a YAML stack to stdout. In noninteractive use, credential values become
`{"$env":"MCPSTACK_SERVER_FIELD"}`. Recognition uses credential field names
(token, secret, password, API/access/private key), authorization/cookie headers,
and named token arguments (`--token VALUE` or `--token=VALUE`). Nested credential
fields and matching environment variable names are covered. Commands, URLs,
ordinary arguments, timeouts, booleans, and existing environment-name settings
remain unchanged. This is name-based detection, not a guarantee that arbitrary
unnamed values or credentials embedded in URLs will be recognized.

Nested fields and array indices contribute to reference names. Names are uppercase
ASCII with punctuation replaced by underscores; collisions receive deterministic
numeric suffixes. Existing references are preserved and their names reserved.
Export never reads or sets environment variables. For an inline credential argument,
the referenced environment value must contain the complete `--token=VALUE` argument.

In an interactive terminal, `export codex` asks whether to mask or include each
detected credential, showing progress such as `Secret 1/12`. Masking is selected
by default; Yes/No to all applies the chosen action to the current and remaining
credentials. `--non-interactive` masks all detected credentials without prompting,
including when standard input is redirected. `export codex --expose-secrets`
preserves literal values, including credentials, without prompting.
No export mode changes the source file or logs its values. Server names and field
keys remain visible. `export codex --config <path>` reads an explicit TOML file instead of the default
config. Relative paths resolve from the current working directory.

### Codex config schema

Import conversion and export validate MCP entries against
`src/integrations/codex/mcp.schema.json`, extracted from the current official Codex config
schema. Validation is offline and does not require Codex to be installed. Native
entries preserve supported fields; obsolete inline `bearer_token` and unknown
fields are rejected without exposing values. Refresh the snapshot deliberately
when adding support for schema changes. Portable conversion supports STDIO/HTTP;
safe filesystem writes currently support Unix only.

### Codex import

All Codex commands accept `--config <path>`; otherwise they use
`$CODEX_HOME/config.toml` or `~/.codex/config.toml`. Paths resolve from the current
directory. Import and diff validate the stack against the bundled MCP schema
without running Codex or servers.

| Command | Behavior |
| --- | --- |
| `diff codex stack.yml` | Show all proposed changes; prompt only for missing masked values. |
| `import codex stack.yml` | Offer backup, approve additions/replacements, and apply without printing a diff. |
| `import codex stack.yml -y` | Create a default backup and approve all changes automatically. |
| `import codex stack.yml --dry-run` | Follow secret and server prompts, then show only approved changes without writes. |

Backups default to numbered sibling files (`config.toml.~1~`, `config.toml.~2~`,
etc.) beyond the highest existing generation. The prompt allows another path or
skipping backup. Requested backups must succeed before stack reads or secret
prompts, never overwrite existing files, and remain after later failures,
cancellation, or no-op imports.

Replacements update the complete server definition. Unrelated settings, comments,
and servers are preserved; identical entries do not rewrite the config. Private
writes use a synced temporary file and check the original snapshot before atomic
replacement. Writes support Unix only; previews work on all platforms. Concurrent
editing still has a final check/rename race.

Diffs use the original and proposed file line numbers. Recognized credentials and
resolved secrets are redacted; hidden changes display `<redacted: changed>`.
Comments and unrelated values are omitted from preview context. These previews
are for review, not patch application. Dry-run logs one initial warning and
creates no files or backups. Server removal, installation/download plans, and
prerequisite verification remain unsupported.
