# Everyday workflows

## Share

```sh
mcpstack codex export work.yaml --non-interactive
mcpstack claude diff work.yaml
mcpstack claude import work.yaml
```

`import` merges servers; those absent from the stack stay configured.

## Switch

```sh
mcpstack codex use personal.yaml --dry-run
mcpstack codex use personal.yaml
```

**`use` removes servers absent from the stack.** An empty stack clears them all.
Unrelated settings are preserved. Repeating an identical import or switch writes nothing.

Both harnesses support these commands. See [secrets and permissions](secrets-and-permissions.md)
and [recovery](recovery.md) before sharing or applying changes.
