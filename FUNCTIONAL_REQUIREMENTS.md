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
| Unsupported | Read/write MCP config at the default or an explicit path. |
| Unsupported | Detect the installed Codex version. |
| Unsupported | Enforce the stack's Codex compatibility range before import/merge. [*](#version-compatibility) |
| Unsupported | Preserve unrelated settings/servers; skip identical entries and reject differing ones. |

### CLI

| Implementation | Brief description |
| --- | --- |
| Partial [*](#commands-and-help) | Predictable commands, help, and examples. |
| Done | Export the entire CLI argument schema for AI consumption. [*](#cli-schema) |
| Partial [*](#exit-statuses-and-error-codes) | Stable exit statuses and actionable error codes. |
| Unsupported | Default interactive prompts and explicit noninteractive operation. [*](#interaction) |
| Partial [*](#deterministic-output) | Deterministic stack files, plans, and structured output. |
| Partial [*](#output-formats) | Human-readable output and structured JSON results; diagnostics on stderr. |
| Unsupported | Define a versioned stack format with harness compatibility ranges. |
| Unsupported | Validate schema versions, server definitions, names, and secret references. |
| Unsupported | Export Codex servers with credentials replaced by references. [*](#phase-1-boundaries) |
| Unsupported | Import/merge a stack into Codex without losing supported fields. [*](#phase-1-boundaries) |
| Unsupported | Resolve secret references from environment variables; reject missing values. [*](#secrets) |
| Unsupported | Atomic config writes with restrictive permissions; preserve originals on failure. |
| Unsupported | Test round trips, repeat imports, version compatibility, missing secrets, and write failures. |

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

[*] Stack schema versions and harness versions are independent. A schema-v2 stack
can import/merge into Codex v6 when its declared Codex range includes v6 (e.g.
`>=6.0.0 <7.0.0`) and mcpstack supports schema v2. Reject unsupported schemas,
out-of-range versions, or undetectable harness versions before writing. Versions
here are illustrative; define range syntax before implementation.

### Phase 1 boundaries

[*] Import means merging server definitions into Codex config, not installing or
starting server software. Validate before writing and leave the config unchanged
on invalid input, unsupported fields, unresolved references, or differing entries
with the same name. Compare parsed definitions after resolving references; skip
identical entries without rewriting an unchanged config. Write changes atomically
with restrictive permissions. Keep credentials out of exports and diagnostics.
Choose reference syntax, file format, and supported Codex fields before implementation.
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

Stack format, exact command/flag syntax, supported transports, additional secret providers,
installation mechanisms, and backup retention/recovery.

Remote catalogs, team access controls, and automatic synchronization are later
scope. Publishing and release/deployment automation require explicit authorization.

## Current status

### Commands and help

Help, version, `--schema`, and runnable examples are implemented. Import/export
commands are pending.

### Exit statuses and error codes

CLI exit statuses are 0 for success/help/version, 1 for output failures
(`OUTPUT_ERROR`), and 2 for invalid arguments (`INVALID_ARGUMENT`). Errors for
stack and harness operations are pending.

### Deterministic output

The CLI schema is generated deterministically from command definitions.
Stack files, plans, and structured operation results are pending.

### Output formats

Help, version, and development status use text output; `--schema` emits JSON.
Diagnostics go to stderr without echoing argument values. Structured JSON
operation results are pending.
