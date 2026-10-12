# Failures and recovery

## Start with the diagnostic

Diagnostics go to stderr. Fatal diagnostics remain visible with `--log off`.
Usage errors include help; operation failures include a stable error code.
Use `mcpstack --schema` for the complete [exit-status reference](cli-reference.md#exit-statuses).

| Failure | Next step |
| --- | --- |
| Invalid arguments or unknown server | Read the selected command's `--help`; check exact server names. |
| Invalid stack | Run `mcpstack validate stack.yaml`; check the format and schema version. |
| Missing masked secret | Supply its environment value, or use an interactive command with hidden prompting. |
| Unsupported field or permission conversion | Use a representable stack for the selected target; do not discard policy silently. |
| Approval required | Run interactively, or deliberately use `--non-interactive -y`. |
| Backup failure | Check that the destination is writable and unused. A requested backup must succeed before applying. |
| Configuration changed since planning | Read the latest target and rerun the preview. Do not apply an old plan. |
| Write or recovery failure | Inspect the reported files and backups before retrying. |

## Backups

Backups use numbered sibling names such as `config.toml.~1~` and
`config.toml.~2~`. Existing backups are never overwritten; numbering continues
after the highest existing generation. Invalid input, dry runs, and no-op
operations create no backups.

Codex interactive imports offer a backup, including a different location or the
choice to skip it. Automatically approved imports and changed stack switches
create a numbered backup. Claude backs up changed existing config and permission
files before replacing either one. Backups remain available after later failures.

On Unix, new backup and replacement files use mode `0600`. On Windows, they inherit
destination-directory permissions. Use protected directories when configurations
contain credentials. A backup-write failure can leave a partial backup: inspect it
before treating it as a recovery source.

## Restore deliberately

1. Stop editing the affected client files and inspect the failure message.
2. Inspect the numbered backups locally. Avoid pasting them into logs or chats.
3. Preserve the current files separately if they include later changes you need.
4. Restore the intended backup to the corresponding config path using your file
   tools. For Claude, check both the server config and permission settings file.
5. Rerun `list` and a fresh preview before making another change.

There is no mcpstack restore command or automatic backup-retention policy.

## Transaction boundaries

Whole-file/stack transactions are the only apply mode. mcpstack validates and
prepares approved changes before committing. Codex uses an atomic config-file
replacement. Claude coordinates its server config and optional settings file;
if a write fails, it attempts to restore earlier changes in reverse order.

Recovery refuses to overwrite detected concurrent edits. An incomplete recovery
is reported explicitly, with guidance to inspect the files and backups.
This is an in-process transaction: it does not provide crash recovery, locking,
or an atomic multi-file commit. A final snapshot-check/replacement race remains.
