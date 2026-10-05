# Codex import

The read-only importer converts schema-v1 stacks to Codex MCP definitions. The
CLI and filesystem merge are delivered in the dependent safe-import PR.

Import initially requires exactly Codex 0.160.0, the checked adapter version.
Export retains its broader read-only version policy. Expanding write compatibility
requires checking the configuration schema for each additional version.

Supported definitions use STDIO or HTTP, with command, args, env, env_vars, cwd,
url, http_headers, env_http_headers, bearer_token_env_var, enabled, required,
startup_timeout_sec, tool_timeout_sec, enabled_tools, and disabled_tools.
Native definitions for other clients, unknown fields, nulls, SSE, and WebSocket
are rejected rather than dropped. Settings and resolved transport values are
validated before writing. The supported field reference is
https://developers.openai.com/codex/config-reference/.

Portable `{env: NAME}` and native `{"$env": "NAME"}` references resolve from the
local environment, including array elements and nested native objects. Missing,
non-Unicode, empty, and NUL-containing values fail without printing values or
variable names. For an exported inline argument reference, the environment value
must contain the complete argument, for example `--token=VALUE`.

Portable HTTP bearer-token references become Codex's `bearer_token_env_var` after
checking that the variable is available. Native `env_vars`, `env_http_headers`,
and `bearer_token_env_var` remain environment bindings managed by Codex; they are
not secret-reference objects and are not substituted into literals.
