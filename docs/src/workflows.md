# Everyday workflows

## Try it

[Download demo.yaml](examples/demo.yaml), then preview against an unused config path:

```sh
mcpstack validate demo.yaml
mcpstack codex use demo.yaml --config scratch.toml --dry-run --non-interactive
```

Nothing is written or executed. For Claude Code, use `claude` and `scratch.json`.

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
