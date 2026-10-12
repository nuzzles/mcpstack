## Examples

```sh
mcpstack codex use work.yaml --dry-run --non-interactive
mcpstack codex use work.yaml
mcpstack codex use work.yaml --non-interactive -y
```

**Replaces the entire configured server set**, removing every server absent from
the stack, including servers added outside mcpstack. An empty stack clears the set.
Unrelated client settings are preserved. `use` does not accept `--server`.

Preview with `--dry-run` to see additions, modifications, and removals without writes
or backups. Applying asks once for approval; `-y` approves the switch. Use
`--non-interactive` to fail on missing secrets instead of prompting.

Permissions are preserved unless `--include-permissions` is supplied. Changed
configurations receive recovery copies; repeating an identical switch is a no-op.
See [failures and recovery](recovery.md) for transaction boundaries.
