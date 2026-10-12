//! Small, in-process transactions over already validated and serialized files.
use super::{Error, Snapshot, fs};

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TransactionError {
    #[error(transparent)]
    Prepare(#[from] Error),
    #[error("Transaction targets must refer to different files. No configuration was changed.")]
    DuplicateTarget,
    #[error("Configuration write failed; earlier transaction writes were restored. {0}")]
    Apply(Error),
    #[error(
        "Configuration write failed and recovery was incomplete because a file changed or could not be restored. Inspect the target files and numbered backups before retrying."
    )]
    Recovery,
}

/// Check every snapshot and back up changed existing files before replacing any.
/// Roll back successful writes on failure, but only if our written bytes remain.
/// A None proposal guards a snapshot without creating or replacing its file.
/// This is not crash recovery or an atomic operation across multiple files.
pub async fn replace_all(writes: Vec<(Snapshot, Option<Vec<u8>>)>) -> Result<(), TransactionError> {
    replace_all_with(writes, |_| Ok(())).await
}

// The private callback makes failures between writes deterministic in tests.
async fn replace_all_with(
    writes: Vec<(Snapshot, Option<Vec<u8>>)>,
    mut before_write: impl FnMut(usize) -> Result<(), Error>,
) -> Result<(), TransactionError> {
    #[cfg(any(unix, windows))]
    let mut targets: Vec<std::path::PathBuf> = Vec::new();
    for (file, _) in &writes {
        file.ensure_write_supported()?;
        file.check_unchanged().await?;
        #[cfg(any(unix, windows))]
        {
            let target = super::file_identity(&file.path)
                .await
                .map_err(|_| Error::Read)?;
            if targets.iter().any(|path| super::same_path(path, &target)) {
                return Err(TransactionError::DuplicateTarget);
            }
            targets.push(target);
        }
    }
    for (file, bytes) in &writes {
        if let Some(bytes) = bytes
            && file.exists()
            && file.contents() != bytes
        {
            file.create_backup_at(&file.default_backup_path().await?)
                .await?;
        }
    }
    // Backup creation can take time; check the entire set again before committing.
    for (file, _) in &writes {
        file.check_unchanged().await?;
    }
    let mut committed: Vec<(&Snapshot, &[u8])> = Vec::new();
    for (index, (file, bytes)) in writes.iter().enumerate() {
        let result = match before_write(index) {
            Ok(()) => match bytes {
                Some(bytes) => file.replace(bytes).await,
                None => file.check_unchanged().await.map(|()| false),
            },
            Err(error) => Err(error),
        };
        match result {
            Ok(true) => {
                if let Some(bytes) = bytes {
                    committed.push((file, bytes));
                }
            }
            Ok(false) => {}
            Err(error) => {
                let mut recovered = true;
                for (file, bytes) in committed.into_iter().rev() {
                    if restore(file, bytes).await.is_err() {
                        recovered = false;
                    }
                }
                return Err(if recovered {
                    TransactionError::Apply(error)
                } else {
                    TransactionError::Recovery
                });
            }
        }
    }
    Ok(())
}

async fn restore(file: &Snapshot, written: &[u8]) -> Result<(), Error> {
    // Never take a fresh snapshot as the baseline: it might contain another writer's edit.
    let expected = Snapshot {
        path: file.path.clone(),
        original: Some(written.to_vec()),
    };
    if let Some(original) = &file.original {
        expected.replace(original).await?;
    } else {
        expected.check_unchanged().await?;
        #[cfg(windows)]
        let path = super::windows::normalize_path(&file.path).map_err(|_| Error::Write)?;
        #[cfg(not(windows))]
        let path = &file.path;
        fs::remove_file(path).await.map_err(|_| Error::Write)?;
    }
    Ok(())
}

#[cfg(all(test, any(unix, windows)))]
mod tests {
    use super::*;
    use std::{fs, path::Path};

    async fn change(path: &Path, bytes: &[u8]) -> (Snapshot, Option<Vec<u8>>) {
        (Snapshot::read(path).await.unwrap(), Some(bytes.to_vec()))
    }

    #[tokio::test]
    async fn success_backs_up_all_changed_files_before_writing_and_repeats_without_backups() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a");
        let b = dir.path().join("b");
        fs::write(&a, b"original a\0\xff").unwrap();
        fs::write(&b, b"original b").unwrap();
        let writes = vec![change(&a, b"new a").await, change(&b, b"new b").await];
        replace_all_with(writes, |index| {
            if index == 0 {
                assert_eq!(
                    fs::read(dir.path().join("a.~1~")).unwrap(),
                    b"original a\0\xff"
                );
                assert_eq!(fs::read(dir.path().join("b.~1~")).unwrap(), b"original b");
                assert_eq!(fs::read(&a).unwrap(), b"original a\0\xff");
            }
            Ok(())
        })
        .await
        .unwrap();
        assert_eq!(fs::read(&a).unwrap(), b"new a");
        assert_eq!(fs::read(&b).unwrap(), b"new b");
        replace_all(vec![change(&a, b"new a").await, change(&b, b"new b").await])
            .await
            .unwrap();
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 4);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for name in ["a", "b", "a.~1~", "b.~1~"] {
                assert_eq!(
                    fs::metadata(dir.path().join(name))
                        .unwrap()
                        .permissions()
                        .mode()
                        & 0o777,
                    0o600
                );
            }
        }
    }

    #[tokio::test]
    async fn failed_second_write_restores_bytes_or_original_absence() {
        for original in [None, Some(&b""[..]), Some(&b"original\0\xff"[..])] {
            let dir = tempfile::tempdir().unwrap();
            let a = dir.path().join("a");
            let b = dir.path().join("b");
            if let Some(bytes) = original {
                fs::write(&a, bytes).unwrap();
            }
            fs::write(&b, b"settings").unwrap();
            let result = replace_all_with(
                vec![change(&a, b"new a").await, change(&b, b"new b").await],
                |index| {
                    if index == 1 {
                        assert_eq!(fs::read(&a).unwrap(), b"new a");
                        Err(Error::Write)
                    } else {
                        Ok(())
                    }
                },
            )
            .await;
            assert_eq!(result, Err(TransactionError::Apply(Error::Write)));
            assert_eq!(fs::read(&a).ok().as_deref(), original);
            assert_eq!(fs::read(&b).unwrap(), b"settings");
            assert_eq!(
                fs::read_dir(dir.path()).unwrap().count(),
                if original.is_some() { 4 } else { 2 }
            );
        }
    }

    #[tokio::test]
    async fn stale_snapshots_and_duplicate_targets_fail_before_backups() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a");
        let b = dir.path().join("b");
        fs::write(&a, b"a").unwrap();
        fs::write(&b, b"b").unwrap();
        let writes = vec![
            change(&a, b"new").await,
            (Snapshot::read(&b).await.unwrap(), None),
        ];
        fs::write(&b, b"external").unwrap();
        assert_eq!(
            replace_all(writes).await,
            Err(TransactionError::Prepare(Error::Changed))
        );
        let writes = vec![
            change(&a, b"new").await,
            change(&dir.path().join("./a"), b"other").await,
        ];
        assert_eq!(
            replace_all(writes).await,
            Err(TransactionError::DuplicateTarget)
        );
        assert_eq!(fs::read(&a).unwrap(), b"a");
        assert_eq!(fs::read(&b).unwrap(), b"external");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
    }

    #[tokio::test]
    async fn later_backup_failure_leaves_all_targets_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a");
        let b = dir.path().join("b");
        fs::write(&a, b"a").unwrap();
        fs::write(&b, b"b").unwrap();
        // Exhaust the next generation for the second file deterministically.
        fs::write(dir.path().join(format!("b.~{}~", u64::MAX)), b"keep").unwrap();
        assert_eq!(
            replace_all(vec![change(&a, b"new").await, change(&b, b"new").await]).await,
            Err(TransactionError::Prepare(Error::Backup))
        );
        assert_eq!(fs::read(&a).unwrap(), b"a");
        assert_eq!(fs::read(&b).unwrap(), b"b");
        assert_eq!(fs::read(dir.path().join("a.~1~")).unwrap(), b"a");
    }

    #[tokio::test]
    async fn concurrent_edit_between_writes_aborts_and_restores_earlier_write() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a");
        let b = dir.path().join("b");
        fs::write(&a, b"a").unwrap();
        fs::write(&b, b"b").unwrap();
        let result = replace_all_with(
            vec![change(&a, b"new").await, change(&b, b"new").await],
            |index| {
                if index == 1 {
                    fs::write(&b, b"external").unwrap();
                }
                Ok(())
            },
        )
        .await;
        assert_eq!(result, Err(TransactionError::Apply(Error::Changed)));
        assert_eq!(fs::read(&a).unwrap(), b"a");
        assert_eq!(fs::read(&b).unwrap(), b"external");
    }

    #[tokio::test]
    async fn recovery_preserves_concurrent_edits_and_still_restores_other_files() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a");
        let b = dir.path().join("b");
        let c = dir.path().join("c");
        fs::write(&a, b"a").unwrap();
        // b starts absent: recovery must not delete a concurrent writer's file.
        let result = replace_all_with(
            vec![
                change(&a, b"new a").await,
                change(&b, b"new b").await,
                change(&c, b"new c").await,
            ],
            |index| {
                if index == 2 {
                    fs::write(&b, b"external").unwrap();
                    Err(Error::Write)
                } else {
                    Ok(())
                }
            },
        )
        .await;
        assert_eq!(result, Err(TransactionError::Recovery));
        assert_eq!(fs::read(&a).unwrap(), b"a");
        assert_eq!(fs::read(&b).unwrap(), b"external");
        assert!(!c.exists());
    }

    #[tokio::test]
    async fn absent_guards_do_not_create_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("absent");
        replace_all(vec![(Snapshot::read(&path).await.unwrap(), None)])
            .await
            .unwrap();
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    }
}
