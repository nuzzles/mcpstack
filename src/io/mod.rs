//! Private, format-independent snapshots, backups, and atomic file replacement.
//! Callers own parsing and merge policy; this module never interprets file contents.
#![deny(unsafe_code)]

#[cfg(unix)]
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::io::AsyncWriteExt;

use thiserror::Error;

#[cfg(windows)]
#[allow(unsafe_code)] // Only the Windows backend may call native security/path APIs.
mod windows;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum Error {
    #[error(
        "Cannot create private backup at the chosen path. Choose an unused file path and check permissions."
    )]
    Backup,
    #[error("Cannot read a regular file at the target path.")]
    Read,
    #[error("The target changed since it was read. No changes were written.")]
    Changed,
    #[error("Unable to write the file atomically. The original was not replaced.")]
    Write,
    #[error("Private file writes are not supported on this platform.")]
    #[cfg(not(any(unix, windows)))]
    Platform,
}

pub struct Snapshot {
    path: PathBuf,
    original: Option<Vec<u8>>,
}

impl Snapshot {
    /// Guard every write path, including callers that do not request a backup.
    pub fn ensure_write_supported(&self) -> Result<(), Error> {
        #[cfg(any(unix, windows))]
        {
            Ok(())
        }
        #[cfg(not(any(unix, windows)))]
        {
            Err(Error::Platform)
        }
    }

    /// Read a plan snapshot without creating files on any platform.
    pub async fn read(path: &Path) -> Result<Self, Error> {
        Ok(Self {
            path: path.into(),
            original: read_regular(path).await.map_err(|_| Error::Read)?,
        })
    }

    #[cfg(all(test, any(unix, windows)))]
    pub async fn backup(path: &Path) -> Result<Self, Error> {
        let snapshot = Self::read(path).await.map_err(|_| Error::Backup)?;
        snapshot.create_backup().await?;
        Ok(snapshot)
    }

    /// Use GNU-style numbered backups, retaining every existing generation.
    pub async fn default_backup_path(&self) -> Result<PathBuf, Error> {
        let parent = self
            .path
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let mut prefix = self.path.file_name().ok_or(Error::Backup)?.to_os_string();
        prefix.push(".~");
        let mut latest = 0_u64;
        let mut entries = fs::read_dir(parent).await.map_err(|_| Error::Backup)?;
        while let Some(entry) = entries.next_entry().await.map_err(|_| Error::Backup)? {
            let name = entry.file_name();
            let Some(number) = name
                .as_encoded_bytes()
                .rsplit(|byte| *byte == b'.')
                .next()
                .and_then(|suffix| suffix.strip_prefix(b"~"))
                .and_then(|suffix| suffix.strip_suffix(b"~"))
                .and_then(|bytes| std::str::from_utf8(bytes).ok())
                .filter(|number| {
                    !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit())
                })
            else {
                continue;
            };
            let mut expected = prefix.clone();
            expected.push(number);
            expected.push("~");
            if !same_path(Path::new(&name), Path::new(&expected)) {
                continue;
            }
            latest = latest.max(number.parse::<u64>().map_err(|_| Error::Backup)?);
        }
        let next = latest.checked_add(1).ok_or(Error::Backup)?;
        let mut path = self.path.as_os_str().to_os_string();
        path.push(format!(".~{next}~"));
        Ok(path.into())
    }

    #[cfg(all(test, any(unix, windows)))]
    pub async fn create_backup(&self) -> Result<(), Error> {
        self.create_backup_at(&self.default_backup_path().await?)
            .await
    }

    /// Create a private recovery copy of the original bytes without overwriting files.
    pub async fn create_backup_at(&self, backup_path: &Path) -> Result<(), Error> {
        #[cfg(not(any(unix, windows)))]
        {
            let _ = backup_path;
            Err(Error::Platform)
        }
        #[cfg(any(unix, windows))]
        {
            self.check_unchanged().await?;
            let target = file_identity(&self.path).await.map_err(|_| Error::Backup)?;
            let backup = file_identity(backup_path)
                .await
                .map_err(|_| Error::Backup)?;
            if same_path(&target, &backup) {
                return Err(Error::Backup);
            }
            let backup_path = backup_path.to_path_buf();
            let file = tokio::task::spawn_blocking(move || create_private(&backup_path))
                .await
                .map_err(|_| Error::Backup)?
                .map_err(|_| Error::Backup)?;
            let mut backup = fs::File::from_std(file);
            // Retain a partial private backup on failure rather than deleting
            // a path another process might have replaced.
            backup
                .write_all(self.original.as_deref().unwrap_or_default())
                .await
                .map_err(|_| Error::Backup)?;
            backup.flush().await.map_err(|_| Error::Backup)?;
            backup.sync_all().await.map_err(|_| Error::Backup)?;
            Ok(())
        }
    }

    /// An absent file is represented as empty input for the caller's parser.
    pub fn contents(&self) -> &[u8] {
        self.original.as_deref().unwrap_or_default()
    }

    /// Replace the target with already-serialized bytes. Returns false for an
    /// unchanged existing file; an absent file can still be created empty.
    pub async fn replace(self, contents: &[u8]) -> Result<bool, Error> {
        self.ensure_write_supported()?;
        self.check_unchanged().await?;
        if self.original.as_deref() == Some(contents) {
            return Ok(false);
        }
        #[cfg(windows)]
        let normalized = windows::normalize_path(&self.path).map_err(|_| Error::Write)?;
        #[cfg(windows)]
        let path = normalized.as_path();
        #[cfg(not(windows))]
        let path = self.path.as_path();
        let parent = path
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let parent = parent.to_path_buf();
        let temporary = tokio::task::spawn_blocking(move || {
            tempfile::Builder::new().make_in(parent, create_private)
        })
        .await
        .map_err(|_| Error::Write)?
        .map_err(|_| Error::Write)?;
        let (file, temporary_path) = temporary.into_parts();
        let mut file = fs::File::from_std(file);
        file.write_all(contents).await.map_err(|_| Error::Write)?;
        file.flush().await.map_err(|_| Error::Write)?;
        file.sync_all().await.map_err(|_| Error::Write)?;
        let file = file.into_std().await;
        self.check_unchanged().await?;
        let path = path.to_path_buf();
        let existed = self.original.is_some();
        // Keep the file and temporary-path guard owned by the blocking operation,
        // including if its awaiting future is dropped while persistence is running.
        tokio::task::spawn_blocking(move || {
            let temporary = tempfile::NamedTempFile::from_parts(file, temporary_path);
            if existed {
                temporary.persist(path)
            } else {
                temporary.persist_noclobber(path)
            }
            .map(|_| ())
            .map_err(|_| Error::Write)
        })
        .await
        .map_err(|_| Error::Write)??;
        Ok(true)
    }

    /// Reject a stale snapshot even when the caller decides no write is needed.
    pub async fn check_unchanged(&self) -> Result<(), Error> {
        if read_regular(&self.path).await.map_err(|_| Error::Changed)? != self.original {
            return Err(Error::Changed);
        }
        Ok(())
    }
}

#[cfg(unix)]
fn create_private(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
}

#[cfg(windows)]
use windows::{create_private, same_path};

#[cfg(not(any(unix, windows)))]
fn create_private(_path: &Path) -> std::io::Result<std::fs::File> {
    Err(std::io::Error::other("private writes are unsupported"))
}

#[cfg(not(windows))]
fn same_path(left: &Path, right: &Path) -> bool {
    left == right
}

#[cfg(any(unix, windows))]
async fn file_identity(path: &Path) -> Result<PathBuf, std::io::Error> {
    #[cfg(windows)]
    windows::validate_path(path)?;
    let parent = path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let name = path
        .file_name()
        .ok_or_else(|| std::io::Error::other("missing filename"))?;
    Ok(fs::canonicalize(parent).await?.join(name))
}

async fn read_regular(path: &Path) -> Result<Option<Vec<u8>>, std::io::Error> {
    #[cfg(windows)]
    windows::validate_path(path)?;
    match fs::symlink_metadata(path).await {
        Ok(metadata) if metadata.file_type().is_file() => {
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
                if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
                    return Err(std::io::Error::other("not a regular file"));
                }
            }
            fs::read(path).await.map(Some)
        }
        Ok(_) => Err(std::io::Error::other("not a regular file")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

#[cfg(all(test, any(unix, windows)))]
mod tests;

#[cfg(all(test, not(any(unix, windows))))]
mod platform_tests {
    use super::*;
    use std::fs;

    #[tokio::test]
    async fn apply_without_backup_refuses_unsupported_private_writes() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.bin");
        let original = "# original\n";
        fs::write(&path, original).unwrap();
        let snapshot = Snapshot::read(&path).await.unwrap();
        assert_eq!(snapshot.ensure_write_supported(), Err(Error::Platform));
        assert_eq!(snapshot.replace(b"new bytes").await, Err(Error::Platform));
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    }
}
