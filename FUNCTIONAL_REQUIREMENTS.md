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
| Unsupported | Read MCP config at an explicit path for export. |
| Unsupported | Write MCP config at the default or an explicit path for import. |
| Done | Detect the installed Codex version. |
| Partial [*](#codex-version-adapters) | Select a Codex adapter supporting the installed version; reject missing adapters or unsupported fields before writes. [*](#version-compatibility) |
| Unsupported | On import, preserve unrelated settings/servers; skip identical entries and reject differing ones. |

### CLI

| Implementation | Brief description |
| --- | --- |
| Partial [*](#commands-and-help) | Predictable commands, help, and examples. |
| Done | Export the entire CLI argument schema for AI consumption. [*](#cli-schema) |
| Partial [*](#exit-statuses-and-error-codes) | Stable exit statuses and actionable error codes. |
| Unsupported | Default interactive prompts and explicit noninteractive operation. [*](#interaction) |
| Partial [*](#deterministic-output) | Deterministic stack files, plans, and structured output. |
| Partial [*](#output-formats) | Human-readable output and structured JSON results; diagnostics on stderr. |
| Done | Define a versioned, client-independent stack format. |
| Done | Validate schema versions, server definitions, names, and secret references. |
| Done | Export Codex server entries as a YAML stack to stdout, preserving values as-is. [*](#codex-export) |
| Unsupported | Replace exported credentials with secret references. |
| Unsupported | Import/merge a stack into Codex without losing supported fields. [*](#phase-1-boundaries) |
| Unsupported | Resolve secret references from environment variables; reject missing values. [*](#secrets) |
| Unsupported | Create a `.bak` copy of the target config before beginning import; abort if backup creation fails. |
| Unsupported | Atomic config writes with restrictive permissions; preserve originals on failure. |
| Partial [*](#stack-workflow-tests) | Test round trips, repeat imports, version compatibility, missing secrets, and write failures. |

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
| Unsupported | Keep discovery, validation, and planning free of writes and server execution. |
| Unsupported | Preview additions, updates, removals, conflicts, and blockers. |
| Unsupported | Show planned writes, downloads, and commands with secrets redacted. |
| Unsupported | Resolve relative paths from the stack location; identify the target config. |
| Unsupported | Apply a whole stack as one transaction. [*](#transactions) |
| Unsupported | Optionally continue installing independent servers after failures. [*](#continue-on-failure) |
| Unsupported | Require explicit conflict resolution and validate prerequisites before writes. |
| Unsupported | Private recovery copies and stale-plan detection. |
| Unsupported | Repeat installs without duplicates or unnecessary side effects. |
| Unsupported | Define supported installation mechanisms; reject unsupported ones before writes. |
| Unsupported | Verify servers through MCP connection and initialization. |
| Unsupported | Bound verification time; report per-server and overall results. |
| Unsupported | Diagnose program, credential, authentication, transport, and protocol failures. |
| Unsupported | Clean up verification processes/connections without invoking application tools. |
| Unsupported | Add secret resolution providers beyond environment variables. [*](#secrets) |
| Unsupported | Keep secrets out of output, shared files, CLI arguments, and child-process logs. |
| Unsupported | Test secret redaction and configuration preservation on success and failure. |
| Unsupported | Test sharing a stack between users with different credentials end to end. |
| Unsupported | Document human and AI workflows, failure handling, switching, and repeat installs. |

## Notes

### Version compatibility

[*] Stack schema versions and harness versions are independent. Shared stacks
carry only their schema version; client compatibility is owned by mcpstack's
adapters. Before import/merge, detect the target client and installed version and
delegate to an adapter supporting that version. Adapters can support a range of
versions when the client's configuration format is stable. Reject unsupported
stack schemas, undetectable client versions, missing adapters, or fields the
selected adapter cannot represent before writing. The adapter also owns reading
for export and preserving unrelated client settings during import.

### Phase 1 boundaries

[*] After locating the target config, create a sibling `config.toml.bak` before
any import processing or changes. If backup creation fails, abort the import.
Do not overwrite an existing backup silently. This requirement applies to import;
read-only export does not create backups.

Import means merging server definitions into Codex config, not installing or
starting server software. Validate before writing and leave the config unchanged
on invalid input, unsupported fields, unresolved references, or differing entries
with the same name. Compare parsed definitions after resolving references; skip
identical entries without rewriting an unchanged config. Write changes atomically
with restrictive permissions. Keep credentials out of exports and diagnostics.
Existing export files require explicit overwrite.

---

### CLI schema

[*] `--schema` emits a versioned JSON description of every implemented command,
argument, option, type, default, requirement, constraint, and example. An AI can
read it without scraping help text. Generate it from the command definitions to
avoid drift. Schema output is independent of future structured operation results
and interactive controls.

### Interaction

[*] Prompt by default in interactive terminals. Explicit noninteractive mode and
redirected input must never prompt or hang; missing inputs produce actionable
errors. Machine-readable results must remain parseable during interactive use.

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

[*] Phase 1 resolves references from environment variables; Phase 2 adds providers.
Document reference syntax. Export must require explicit
classification of ambiguous sensitive values. Shared stacks never contain resolved
credentials; private client configs and recovery copies use restrictive permissions.

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

Help, version, `--schema`, `validate <file>`, `export codex`, and runnable help
examples are implemented. Import commands are pending.

### Exit statuses and error codes

CLI exit statuses are 0 for success/help/version, 1 for output failures
(`OUTPUT_ERROR`), 2 for invalid arguments (`INVALID_ARGUMENT`), 3 for stack read
failures (`STACK_READ_ERROR`), and 4 for invalid stacks (`INVALID_STACK`).
Config read failures use 5 (`CONFIG_READ_ERROR`); export failures use 6
(`EXPORT_ERROR`). Codex detection failures use 7 (`CLIENT_VERSION_ERROR`);
newer 0.x versions warn on stderr and continue export. Codex 1.x and later,
including prereleases, fail with exit 8 (`UNSUPPORTED_CLIENT_VERSION`). Import errors are pending.

### Deterministic output

The CLI schema is generated deterministically from command definitions. Stack v1
uses ordered maps and passes serialization round-trip tests. Codex export emits
deterministic native YAML stacks. Plans and structured operation results are pending.

### Output formats

Help, version, and validation results use text output;
`--schema` emits JSON; `export codex` emits YAML.
Diagnostics go to stderr without echoing argument values. Structured JSON
operation results are pending.

### Stack workflow tests

Stack serialization round trips, client-independent documents, reference syntax,
unsupported shared fields, native client fields and secret references, transport
URL schemes, validation file preservation, and CLI output failures are
covered. Codex version parsing, detection failures, and adapter selection are
also covered. Repeat imports, missing secret resolution, and atomic configuration
write failures are pending.


### Codex export

`export codex` reads the default Codex TOML config and prints native server
entries as a YAML stack to stdout, preserving values as-is. It does not change
the source file. Explicit config paths, secret handling, and imports are pending.


### Codex version adapters

`export codex` runs `codex --version` and selects an adapter before reading the
configuration. The native TOML adapter covers versions from 0.0.0 through the
checked stable release 0.160.0, retaining historical field names and values.
Newer 0.x versions (including future 0.x prereleases) warn on stderr and export
using the existing adapter. Codex 1.0.0 and later, including major-version
prereleases, require an explicit adapter and fail before configuration reads. This range describes the reader's policy,
not runtime testing of every historical release; it reads only the configured
TOML file, not legacy non-TOML or effective layered configuration.
Detection failures use exit 7 (`CLIENT_VERSION_ERROR`). Tests use a fake Codex
executable and cover older/current/newer versions, warnings, and detection failures. Version-aware import field validation
and config writes remain pending. Backup creation is required for the future
importer and is not implemented in this step.
