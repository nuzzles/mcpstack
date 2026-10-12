# For agents

Start with [llms.txt](llms.txt) for the documentation index or
[llms-full.txt](llms-full.txt) for all chapters in one Markdown document.
The [CLI schema](cli-schema.json) is generated from the same source revision as
this site. Prefer `mcpstack --schema` when operating an installed version.

## Inspect before applying

```sh
mcpstack --schema
mcpstack codex list --config target.toml
mcpstack validate stack.yaml
mcpstack codex diff stack.yaml --config target.toml --non-interactive --color never
mcpstack codex import stack.yaml --config target.toml --dry-run --non-interactive -y --color never
```

Use an existing target file for `list`. Substitute `claude` and a JSON target
for Claude Code. Pass explicit paths when working with a test configuration.
Discovery and preview commands do not execute servers or write configuration.

Once the intended changes are authorized:

```sh
mcpstack codex import stack.yaml --config target.toml --non-interactive -y --color never
```

For a whole-stack replacement, preview with `use`, then apply with `use`:

```sh
mcpstack codex use stack.yaml --config target.toml --dry-run --non-interactive --color never
mcpstack codex use stack.yaml --config target.toml --non-interactive -y --color never
```

`use` removes servers absent from the stack. An import diff does not show those
removals. Permission changes require a separate, explicit `--include-permissions`
on both preview and apply. Keep the same target, selection, and permission flags.

## Avoid unexpected prompts

Use `--non-interactive` even when your process has a terminal. `-y` approves
changes but, on its own, does not suppress missing-secret prompts during previews.
Missing masked literals fail without prompting in noninteractive mode. Supply
them through the environment, not command-line values or captured logs.

For exports, `--non-interactive` selects masking. Replacing an existing output file
still requires an explicit `--overwrite`.

## Handle the output contract

| Stream/result | Meaning |
| --- | --- |
| `--schema` stdout | One versioned JSON document describing the CLI and exit statuses. |
| `show` stdout | YAML stack data; diagnostics remain on stderr. |
| Other stdout | Human-readable plans, diffs, inventory, and operation results. |
| stderr | Diagnostics, warnings, logs, and interactive prompts. |
| Nonzero exit status | Failure; inspect the diagnostic and preserve the original plan for review. |

There is no JSON result mode for import, use, or list. Do not treat display text
as a versioned machine API. Use the exit status and the documented output format.
Interactive cancellations can exit successfully with an explicit cancellation
message; exit zero alone does not prove a write occurred. No-op results also
exit successfully without rewriting files.

Do not retry a failed apply blindly. A stale plan needs a fresh preview; a failed
recovery needs inspection. See [Failures and recovery](recovery.md).
