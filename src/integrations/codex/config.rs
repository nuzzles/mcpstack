//! Codex TOML comparison and merge policy over shared file I/O.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use thiserror::Error;
use toml_edit::{DocumentMut, Item, Table};

use crate::io;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum FileError {
    #[error(
        "Cannot create backup at the chosen path. Choose an unused file path and check permissions. Import was not processed."
    )]
    Backup,
    #[error("Invalid target TOML or mcp_servers table. Client configuration was not changed.")]
    Config,
    #[error("The target changed during import. No config changes were written; retry the import.")]
    Changed,
    #[error("Unable to write config atomically. The original config was not replaced.")]
    Write,
    #[error("Config writes are not supported on this platform.")]
    #[cfg(not(any(unix, windows)))]
    Platform,
}

impl From<io::Error> for FileError {
    fn from(error: io::Error) -> Self {
        match error {
            io::Error::Read => Self::Config,
            io::Error::Backup => Self::Backup,
            io::Error::Changed => Self::Changed,
            io::Error::Write => Self::Write,
            #[cfg(not(any(unix, windows)))]
            io::Error::Platform => Self::Platform,
        }
    }
}

pub struct Snapshot {
    file: io::Snapshot,
}

impl Snapshot {
    pub async fn read(path: &Path) -> Result<Self, FileError> {
        Ok(Self {
            file: io::Snapshot::read(path).await?,
        })
    }

    pub fn ensure_write_supported(&self) -> Result<(), FileError> {
        self.file.ensure_write_supported().map_err(Into::into)
    }

    pub async fn default_backup_path(&self) -> Result<PathBuf, FileError> {
        self.file.default_backup_path().await.map_err(Into::into)
    }

    pub async fn create_backup_at(&self, path: &Path) -> Result<(), FileError> {
        self.file.create_backup_at(path).await.map_err(Into::into)
    }

    #[cfg(all(test, any(unix, windows)))]
    async fn backup(path: &Path) -> Result<Self, FileError> {
        Ok(Self {
            file: io::Snapshot::backup(path).await?,
        })
    }

    #[cfg(all(test, any(unix, windows)))]
    async fn create_backup(&self) -> Result<(), FileError> {
        self.file.create_backup().await.map_err(Into::into)
    }

    /// Compare additions and replacements without writes.
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
        let text = std::str::from_utf8(self.file.contents()).map_err(|_| FileError::Config)?;
        let document = text.parse::<DocumentMut>().map_err(|_| FileError::Config)?;
        if document.contains_key("mcp_servers") && !document["mcp_servers"].is_table_like() {
            return Err(FileError::Config);
        }
        Ok(document)
    }

    pub fn original_text(&self) -> Result<&str, FileError> {
        std::str::from_utf8(self.file.contents()).map_err(|_| FileError::Config)
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

    pub async fn apply(
        self,
        definitions: &BTreeMap<String, toml::Table>,
    ) -> Result<usize, FileError> {
        self.ensure_write_supported()?;
        let (added, document) = self.proposal(definitions)?;
        if added == 0 {
            self.file.check_unchanged().await?;
        } else {
            self.file.replace(document.as_bytes()).await?;
        }
        Ok(added)
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

#[cfg(all(test, any(unix, windows)))]
mod tests {
    use super::*;
    use std::fs;

    fn definitions(command: &str) -> BTreeMap<String, toml::Table> {
        BTreeMap::from([(
            "added".into(),
            toml::from_str(&format!(
                "command='{command}'\nenabled=true\nrequired=false"
            ))
            .unwrap(),
        )])
    }

    #[tokio::test]
    async fn target_changed_after_preview_aborts_before_backup() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, "# original").unwrap();
        let snapshot = Snapshot::read(&path).await.unwrap();
        assert_eq!(snapshot.preview(&definitions("tool")).unwrap().len(), 1);
        assert!(!path.with_extension("toml.~1~").exists());
        fs::write(&path, "# external edit").unwrap();
        assert_eq!(snapshot.create_backup().await, Err(FileError::Changed));
        assert_eq!(fs::read_to_string(&path).unwrap(), "# external edit");
        assert!(!path.with_extension("toml.~1~").exists());
    }

    #[tokio::test]
    async fn preserves_comments_settings_and_skips_semantically_identical_servers() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let original = "# private settings\nmodel = 'model' # keep\n[mcp_servers.unrelated]\ncommand='other'\n";
        fs::write(&path, original).unwrap();
        assert_eq!(
            Snapshot::backup(&path)
                .await
                .unwrap()
                .apply(&definitions("tool"))
                .await
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
        fs::rename(
            path.with_extension("toml.~1~"),
            dir.path().join("first-backup"),
        )
        .unwrap();
        assert_eq!(
            Snapshot::backup(&path)
                .await
                .unwrap()
                .apply(&definitions("tool"))
                .await
                .unwrap(),
            0
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), written);
        assert_eq!(
            fs::metadata(&path).unwrap().modified().unwrap(),
            metadata.modified().unwrap()
        );
    }

    #[tokio::test]
    async fn defaults_inline_tables_and_quoted_names_compare_correctly() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let original = "mcp_servers = {added={command='tool', args=[], env={}}}\n";
        fs::write(&path, original).unwrap();
        assert_eq!(
            Snapshot::backup(&path)
                .await
                .unwrap()
                .apply(&definitions("tool"))
                .await
                .unwrap(),
            0
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
        fs::remove_file(path.with_extension("toml.~1~")).unwrap();
        let mut incoming = definitions("new");
        let definition = incoming.remove("added").unwrap();
        incoming.insert("quoted.name".into(), definition);
        assert_eq!(
            Snapshot::backup(&path)
                .await
                .unwrap()
                .apply(&incoming)
                .await
                .unwrap(),
            1
        );
        let written: toml::Table = toml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            written["mcp_servers"]["quoted.name"]["command"].as_str(),
            Some("new")
        );
    }

    #[tokio::test]
    async fn backup_conflicts_and_parse_errors_preserve_originals() {
        for original in ["broken = [", "mcp_servers=42"] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("config.toml");
            fs::write(&path, original).unwrap();
            assert!(
                Snapshot::backup(&path)
                    .await
                    .unwrap()
                    .apply(&definitions("tool"))
                    .await
                    .is_err()
            );
            assert_eq!(fs::read_to_string(&path).unwrap(), original);
            assert_eq!(
                fs::read_to_string(path.with_extension("toml.~1~")).unwrap(),
                original
            );
            Snapshot::backup(&path).await.unwrap();
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
}
