Check stack syntax without resolving secrets, reading client config, or running servers.

```sh
mcpstack validate work.yaml
```

A valid stack may contain unsupported target settings. Use [diff](command-diff.md)
to check harness compatibility.
