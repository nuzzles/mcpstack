# Failures and recovery

| Failure | Action |
| --- | --- |
| Invalid stack or unsupported setting | Validate the stack and check harness compatibility. |
| Missing secret | Supply its environment value or run interactively. |
| Configuration changed | Read the latest file and preview again. |
| Write or recovery failed | Inspect the reported files and backups before retrying. |

## Backups

Backups use numbered siblings such as `config.toml.~1~`; existing backups are never
replaced. Dry runs and unchanged configurations create none. Codex interactive imports
can skip backups; automatic imports, stack switches, and Claude writes back up changed
existing files. Keep credential-bearing configs and backups private.

To restore, preserve any newer changes, inspect the backup, then copy it to the original
path. For Claude, check both MCP config and permission settings. There is no restore command.

## Transactions

Changes apply as a whole file/stack. Codex replaces one file atomically. Claude
attempts rollback if either file fails, while protecting detected concurrent edits.
There is no crash recovery, locking, or atomic multi-file commit. Incomplete recovery
is reported; a backup-write failure may leave a partial backup.
