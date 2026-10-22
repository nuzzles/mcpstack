Check stack syntax without resolving secrets, reading client config, or running servers.

```sh
mcpstack validate work.yaml
mcpstack validate https://example.com/work.yaml
```

A valid stack may contain unsupported target settings. Use [diff](command-diff.md)
to check harness compatibility.
URL validation fetches the stack but does not resolve client settings or secrets.
