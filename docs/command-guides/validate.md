## Example

```sh
mcpstack validate work.yaml
```

Checks a YAML stack against the shared format without resolving environment
references, reading client configuration, writing files, or executing servers.
Validation is offline and does not require a harness installation.

A valid stack may still contain settings the target harness cannot represent.
Follow validation with `mcpstack codex diff work.yaml` or
`mcpstack claude diff work.yaml` to check the intended import. See
[supported harnesses](compatibility.md) and the [stack format](stack-format.md).
