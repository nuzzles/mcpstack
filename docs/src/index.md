# Your MCP setup, in a file.

**mcpstack 0.1.0** saves MCP server configurations as portable YAML stacks. Review changes,
merge a few servers, or switch your complete setup between work and personal projects.
Use the same commands with **Codex** and **Claude Code**.

```sh
mcpstack codex export work.yaml --non-interactive
mcpstack claude diff work.yaml
mcpstack claude import work.yaml
```

mcpstack manages client configuration. Server installation and execution are
handled by your MCP client and the server's own setup instructions.

## Start with your workflow

| You want to… | Start here |
| --- | --- |
| Install the tool and try it without touching your configuration | [Getting started](getting-started.md) |
| Export, share, import, or switch setups | [Everyday workflows](workflows.md) |
| Keep credentials local and choose whether to copy permissions | [Secrets and permissions](secrets-and-permissions.md) |
| Understand a failure or restore a backup | [Failures and recovery](recovery.md) |
| Run predictable commands from an agent or script | [For agents](agents.md) |

## Small files. Explicit changes.

```yaml
schema_version: 1
servers:
  example:
    transport:
      type: http
      url: https://mcp.example.com/mcp
```

This is an illustrative endpoint, not a running server. A stack describes configuration;
loading or previewing one never runs its servers.

`import` merges selected server definitions. `use` replaces the complete server set,
including removing servers absent from the stack. Both preserve unrelated client
settings. Permission handling is opt-in with `--include-permissions`.

## Read it your way

Use the sidebar or search to browse. Every chapter has a **Read Markdown** link.
Agents can begin with [llms.txt](llms.txt), read the [complete Markdown guide](llms-full.txt),
or inspect the generated [CLI schema](cli-schema.json).

This guide covers mcpstack **0.1.0**. Run `mcpstack --version` to check your version;
your binary's `--help` and `--schema` describe its available commands.
