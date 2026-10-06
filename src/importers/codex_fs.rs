//! Backup-first, preserving, atomic Codex config merges.
use std::collections::BTreeMap;
use std::fs;
#[cfg(unix)]
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

use thiserror::Error;
use toml_edit::{DocumentMut, Item, Table};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum FileError {
    #[error(
        "Cannot create private backup at the chosen path. Choose an unused file path and check permissions. Import was not processed."
    )]
    Backup,
    #[error("Invalid target TOML or mcp_servers table. Original config and backup are unchanged.")]
    Config,
    #[error(
        "The target changed during import. No config changes were written; retry with a fresh backup."
    )]
    Changed,
    #[error(
        "Unable to write config atomically. The original config and private backup remain available."
    )]
    Write,
    #[error(
        "Private config writes currently require Unix permissions. This platform is not supported for import."
    )]
    #[cfg(not(unix))]
    Platform,
}

pub struct Snapshot {
    path: PathBuf,
    original: Option<Vec<u8>>,
}

impl Snapshot {
    /// Read a plan snapshot without creating files, including on non-Unix hosts.
    pub fn read(path: &Path) -> Result<Self, FileError> {
        Ok(Self {
            path: path.into(),
            original: read_regular(path).map_err(|_| FileError::Config)?,
        })
    }

    #[cfg(all(test, unix))]
    pub fn backup(path: &Path) -> Result<Self, FileError> {
        let snapshot = Self::read(path).map_err(|_| FileError::Backup)?;
        snapshot.create_backup()?;
        Ok(snapshot)
    }

    /// Use GNU-style numbered backups, retaining every existing generation.
    pub fn default_backup_path(&self) -> Result<PathBuf, FileError> {
        let parent = self
            .path
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let mut prefix = self
            .path
            .file_name()
            .ok_or(FileError::Backup)?
            .to_os_string();
        prefix.push(".~");
        let mut latest = 0_u64;
        for entry in fs::read_dir(parent).map_err(|_| FileError::Backup)? {
            let name = entry.map_err(|_| FileError::Backup)?.file_name();
            let Some(suffix) = name
                .as_encoded_bytes()
                .strip_prefix(prefix.as_encoded_bytes())
            else {
                continue;
            };
            let Some(number) = suffix
                .strip_suffix(b"~")
                .and_then(|bytes| std::str::from_utf8(bytes).ok())
                .filter(|number| {
                    !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit())
                })
            else {
                continue;
            };
            latest = latest.max(number.parse::<u64>().map_err(|_| FileError::Backup)?);
        }
        let next = latest.checked_add(1).ok_or(FileError::Backup)?;
        let mut path = self.path.as_os_str().to_os_string();
        path.push(format!(".~{next}~"));
        Ok(path.into())
    }

    #[cfg(all(test, unix))]
    pub fn create_backup(&self) -> Result<(), FileError> {
        self.create_backup_at(&self.default_backup_path()?)
    }

    /// Back up the original before reading the stack or resolving secrets.
    pub fn create_backup_at(&self, backup_path: &Path) -> Result<(), FileError> {
        #[cfg(not(unix))]
        {
            let _ = backup_path;
            Err(FileError::Platform)
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            self.check_unchanged()?;
            if file_identity(&self.path).map_err(|_| FileError::Backup)?
                == file_identity(backup_path).map_err(|_| FileError::Backup)?
            {
                return Err(FileError::Backup);
            }
            let mut backup = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(backup_path)
                .map_err(|_| FileError::Backup)?;
            // Retain a partial private backup on failure rather than deleting
            // a path another process might have replaced.
            backup
                .write_all(self.original.as_deref().unwrap_or_default())
                .map_err(|_| FileError::Backup)?;
            backup.sync_all().map_err(|_| FileError::Backup)?;
            Ok(())
        }
    }

    /// Compare without writes so dry-run can show conflicts as well as additions.
    pub fn preview(
        &self,
        definitions: &BTreeMap<String, toml::Table>,
    ) -> Result<BTreeMap<String, (Option<toml::Table>, toml::Table)>, FileError> {
        let document = self.document()?;
        let mut changes = BTreeMap::new();
        for (name, definition) in definitions {
            let existing = document
                .get("mcp_servers")
                .and_then(Item::as_table_like)
                .and_then(|servers| servers.get(name))
                .map(existing_table)
                .transpose()?;
            if existing.as_ref().is_some_and(|existing| {
                canonical(existing.clone()) == canonical(definition.clone())
            }) {
                continue;
            }
            changes.insert(name.clone(), (existing, definition.clone()));
        }
        Ok(changes)
    }

    fn document(&self) -> Result<DocumentMut, FileError> {
        let text = std::str::from_utf8(self.original.as_deref().unwrap_or_default())
            .map_err(|_| FileError::Config)?;
        let document = text.parse::<DocumentMut>().map_err(|_| FileError::Config)?;
        if document.contains_key("mcp_servers") && !document["mcp_servers"].is_table_like() {
            return Err(FileError::Config);
        }
        Ok(document)
    }

    pub fn original_text(&self) -> Result<&str, FileError> {
        std::str::from_utf8(self.original.as_deref().unwrap_or_default())
            .map_err(|_| FileError::Config)
    }

    /// Build exactly the document that apply writes, without touching the filesystem.
    pub fn proposal(
        &self,
        definitions: &BTreeMap<String, toml::Table>,
    ) -> Result<(usize, String), FileError> {
        let additions: BTreeMap<_, _> = self
            .preview(definitions)?
            .into_iter()
            .map(|(name, (_, definition))| (name, definition))
            .collect();
        let mut document = self.document()?;
        let added = additions.len();
        for (name, definition) in &additions {
            if !document.contains_key("mcp_servers") {
                document["mcp_servers"] = Item::Table(Table::new());
            }
            let encoded = toml::to_string(definition).map_err(|_| FileError::Write)?;
            let server = encoded
                .parse::<DocumentMut>()
                .map_err(|_| FileError::Write)?
                .into_table();
            document["mcp_servers"]
                .as_table_like_mut()
                .ok_or(FileError::Config)?
                .insert(name, Item::Table(server));
        }
        Ok((added, document.to_string()))
    }

    pub fn apply(self, definitions: &BTreeMap<String, toml::Table>) -> Result<usize, FileError> {
        let (added, document) = self.proposal(definitions)?;
        self.check_unchanged()?;
        if added == 0 {
            return Ok(0);
        }
        let parent = self
            .path
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let mut temporary =
            tempfile::NamedTempFile::new_in(parent).map_err(|_| FileError::Write)?;
        temporary
            .write_all(document.as_bytes())
            .map_err(|_| FileError::Write)?;
        temporary
            .as_file()
            .sync_all()
            .map_err(|_| FileError::Write)?;
        self.check_unchanged()?;
        if self.original.is_some() {
            temporary
                .persist(&self.path)
                .map_err(|_| FileError::Write)?;
        } else {
            temporary
                .persist_noclobber(&self.path)
                .map_err(|_| FileError::Write)?;
        }
        Ok(added)
    }

    fn check_unchanged(&self) -> Result<(), FileError> {
        if read_regular(&self.path).map_err(|_| FileError::Changed)? != self.original {
            return Err(FileError::Changed);
        }
        Ok(())
    }
}

#[cfg(unix)]
fn file_identity(path: &Path) -> Result<PathBuf, std::io::Error> {
    let parent = path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let name = path
        .file_name()
        .ok_or_else(|| std::io::Error::other("missing filename"))?;
    Ok(parent.canonicalize()?.join(name))
}

fn read_regular(path: &Path) -> Result<Option<Vec<u8>>, std::io::Error> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_file() => fs::read(path).map(Some),
        Ok(_) => Err(std::io::Error::other("not a regular file")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn existing_table(item: &Item) -> Result<toml::Table, FileError> {
    // Parse via a wrapper so inline tables and nested tables share one path.
    let mut wrapper = DocumentMut::new();
    wrapper["server"] = item.clone();
    let mut value: toml::Table =
        toml::from_str(&wrapper.to_string()).map_err(|_| FileError::Config)?;
    match value.remove("server") {
        Some(toml::Value::Table(table)) => Ok(table),
        _ => Err(FileError::Config),
    }
}

fn canonical(mut table: toml::Table) -> toml::Table {
    for (key, default) in [("enabled", true), ("required", false)] {
        table.entry(key).or_insert(toml::Value::Boolean(default));
    }
    for key in ["args", "env_vars", "disabled_tools"] {
        if table
            .get(key)
            .is_some_and(|value| value.as_array().is_some_and(Vec::is_empty))
        {
            table.remove(key);
        }
    }
    for key in ["env", "http_headers", "env_http_headers"] {
        if table
            .get(key)
            .is_some_and(|value| value.as_table().is_some_and(toml::Table::is_empty))
        {
            table.remove(key);
        }
    }
    // TOML integer and floating seconds express the same timeout.
    for key in ["startup_timeout_sec", "tool_timeout_sec"] {
        if let Some(toml::Value::Integer(value)) = table.get(key) {
            table.insert(key.into(), toml::Value::Float(*value as f64));
        }
    }
    table
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};

    fn definitions(command: &str) -> BTreeMap<String, toml::Table> {
        BTreeMap::from([(
            "added".into(),
            toml::from_str(&format!(
                "command='{command}'\nenabled=true\nrequired=false"
            ))
            .unwrap(),
        )])
    }

    #[test]
    fn numbered_backups_preserve_generations_and_skip_occupied_names() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("codex.bak");
        fs::write(&path, "first").unwrap();
        let snapshot = Snapshot::read(&path).unwrap();
        let first = snapshot.default_backup_path().unwrap();
        assert_eq!(first, dir.path().join("codex.bak.~1~"));
        snapshot.create_backup_at(&first).unwrap();
        fs::write(&path, "second").unwrap();
        let snapshot = Snapshot::read(&path).unwrap();
        let second = snapshot.default_backup_path().unwrap();
        assert_eq!(second, dir.path().join("codex.bak.~2~"));
        snapshot.create_backup_at(&second).unwrap();
        assert_eq!(fs::read_to_string(&first).unwrap(), "first");
        assert_eq!(fs::read_to_string(&second).unwrap(), "second");
        // Gaps, directories, and dangling symlinks must not cause reuse.
        fs::remove_file(&first).unwrap();
        fs::create_dir(dir.path().join("codex.bak.~9~")).unwrap();
        symlink(
            dir.path().join("missing"),
            dir.path().join("codex.bak.~10~"),
        )
        .unwrap();
        fs::write(dir.path().join("other.~99~"), "unrelated").unwrap();
        let next = snapshot.default_backup_path().unwrap();
        assert_eq!(next, dir.path().join("codex.bak.~11~"));
        // An intervening creation still cannot be overwritten.
        fs::write(&next, "concurrent backup").unwrap();
        assert_eq!(snapshot.create_backup_at(&next), Err(FileError::Backup));
        assert_eq!(fs::read_to_string(&next).unwrap(), "concurrent backup");
    }

    #[test]
    fn custom_backups_are_private_and_cannot_use_the_target_or_overwrite_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let absent = Snapshot::read(&path).unwrap();
        let alias = dir.path().join(".").join("config.toml");
        assert_eq!(absent.create_backup_at(&alias), Err(FileError::Backup));
        assert!(!path.exists());
        fs::write(&path, "# private fixture").unwrap();
        let snapshot = Snapshot::read(&path).unwrap();
        let custom = dir.path().join("backup with spaces.toml");
        snapshot.create_backup_at(&custom).unwrap();
        assert_eq!(fs::read_to_string(&custom).unwrap(), "# private fixture");
        assert_eq!(
            fs::metadata(&custom).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert!(!snapshot.default_backup_path().unwrap().exists());
        assert_eq!(snapshot.create_backup_at(&custom), Err(FileError::Backup));
        assert_eq!(fs::read_to_string(&custom).unwrap(), "# private fixture");
    }

    #[test]
    fn target_changed_after_preview_aborts_before_backup() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, "# original").unwrap();
        let snapshot = Snapshot::read(&path).unwrap();
        assert_eq!(snapshot.preview(&definitions("tool")).unwrap().len(), 1);
        assert!(!path.with_extension("toml.~1~").exists());
        fs::write(&path, "# external edit").unwrap();
        assert_eq!(snapshot.create_backup(), Err(FileError::Changed));
        assert_eq!(fs::read_to_string(&path).unwrap(), "# external edit");
        assert!(!path.with_extension("toml.~1~").exists());
    }

    #[test]
    fn preserves_comments_settings_and_skips_semantically_identical_servers() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let original = "# private settings\nmodel = 'model' # keep\n[mcp_servers.unrelated]\ncommand='other'\n";
        fs::write(&path, original).unwrap();
        assert_eq!(
            Snapshot::backup(&path)
                .unwrap()
                .apply(&definitions("tool"))
                .unwrap(),
            1
        );
        assert_eq!(
            fs::read(path.with_extension("toml.~1~")).unwrap(),
            original.as_bytes()
        );
        let written = fs::read_to_string(&path).unwrap();
        assert!(written.starts_with(original));
        let metadata = fs::metadata(&path).unwrap();
        assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
        assert_eq!(
            fs::metadata(path.with_extension("toml.~1~"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        fs::rename(
            path.with_extension("toml.~1~"),
            dir.path().join("first-backup"),
        )
        .unwrap();
        assert_eq!(
            Snapshot::backup(&path)
                .unwrap()
                .apply(&definitions("tool"))
                .unwrap(),
            0
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), written);
        assert_eq!(
            fs::metadata(&path).unwrap().modified().unwrap(),
            metadata.modified().unwrap()
        );
    }

    #[test]
    fn defaults_inline_tables_and_quoted_names_compare_correctly() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let original = "mcp_servers = {added={command='tool', args=[], env={}}}\n";
        fs::write(&path, original).unwrap();
        assert_eq!(
            Snapshot::backup(&path)
                .unwrap()
                .apply(&definitions("tool"))
                .unwrap(),
            0
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
        fs::remove_file(path.with_extension("toml.~1~")).unwrap();
        let mut incoming = definitions("new");
        let definition = incoming.remove("added").unwrap();
        incoming.insert("quoted.name".into(), definition);
        assert_eq!(
            Snapshot::backup(&path).unwrap().apply(&incoming).unwrap(),
            1
        );
        let written: toml::Table = toml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            written["mcp_servers"]["quoted.name"]["command"].as_str(),
            Some("new")
        );
    }

    #[test]
    fn backup_conflicts_and_parse_errors_preserve_originals() {
        for original in ["broken = [", "mcp_servers=42"] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("config.toml");
            fs::write(&path, original).unwrap();
            assert!(
                Snapshot::backup(&path)
                    .unwrap()
                    .apply(&definitions("tool"))
                    .is_err()
            );
            assert_eq!(fs::read_to_string(&path).unwrap(), original);
            assert_eq!(
                fs::read_to_string(path.with_extension("toml.~1~")).unwrap(),
                original
            );
            Snapshot::backup(&path).unwrap();
            assert_eq!(
                fs::read_to_string(path.with_extension("toml.~2~")).unwrap(),
                original
            );
            assert_eq!(
                fs::read_to_string(path.with_extension("toml.~1~")).unwrap(),
                original
            );
        }
    }

    #[test]
    fn refuses_symlinks_and_stale_snapshots_and_creates_absent_configs() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let snapshot = Snapshot::backup(&path).unwrap();
        assert_eq!(fs::read(path.with_extension("toml.~1~")).unwrap(), b"");
        assert_eq!(snapshot.apply(&definitions("tool")).unwrap(), 1);
        fs::remove_file(path.with_extension("toml.~1~")).unwrap();
        let snapshot = Snapshot::backup(&path).unwrap();
        fs::write(&path, "# external edit").unwrap();
        assert_eq!(
            snapshot.apply(&definitions("tool")),
            Err(FileError::Changed)
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), "# external edit");
        let link = dir.path().join("link.toml");
        symlink(&path, &link).unwrap();
        assert!(matches!(Snapshot::backup(&link), Err(FileError::Backup)));
        assert!(!link.with_extension("toml.~1~").exists());
    }

    #[test]
    fn temporary_write_failure_leaves_original_and_backup_intact() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, "# original").unwrap();
        let snapshot = Snapshot::backup(&path).unwrap();
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o500)).unwrap();
        // Root can bypass directory permissions; in that environment this failure
        // cannot be induced with permissions, so restore and return.
        if tempfile::NamedTempFile::new_in(dir.path()).is_ok() {
            fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
            return;
        }
        let result = snapshot.apply(&definitions("tool"));
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(result, Err(FileError::Write));
        assert_eq!(fs::read_to_string(&path).unwrap(), "# original");
        assert_eq!(
            fs::read_to_string(path.with_extension("toml.~1~")).unwrap(),
            "# original"
        );
    }
}
