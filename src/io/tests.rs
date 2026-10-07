use super::*;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::{PermissionsExt, symlink};

#[cfg(unix)]
#[tokio::test]
async fn numbered_backups_preserve_generations_and_skip_occupied_names() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.bak");
    fs::write(&path, "first").unwrap();
    let snapshot = Snapshot::read(&path).await.unwrap();
    let first = snapshot.default_backup_path().await.unwrap();
    assert_eq!(first, dir.path().join("settings.bak.~1~"));
    snapshot.create_backup_at(&first).await.unwrap();
    fs::write(&path, "second").unwrap();
    let snapshot = Snapshot::read(&path).await.unwrap();
    let second = snapshot.default_backup_path().await.unwrap();
    assert_eq!(second, dir.path().join("settings.bak.~2~"));
    snapshot.create_backup_at(&second).await.unwrap();
    assert_eq!(fs::read_to_string(&first).unwrap(), "first");
    assert_eq!(fs::read_to_string(&second).unwrap(), "second");
    // Gaps, directories, and dangling symlinks must not cause reuse.
    fs::remove_file(&first).unwrap();
    fs::create_dir(dir.path().join("settings.bak.~9~")).unwrap();
    symlink(
        dir.path().join("missing"),
        dir.path().join("settings.bak.~10~"),
    )
    .unwrap();
    fs::write(dir.path().join("other.~99~"), "unrelated").unwrap();
    let next = snapshot.default_backup_path().await.unwrap();
    assert_eq!(next, dir.path().join("settings.bak.~11~"));
    // An intervening creation still cannot be overwritten.
    fs::write(&next, "concurrent backup").unwrap();
    assert_eq!(snapshot.create_backup_at(&next).await, Err(Error::Backup));
    assert_eq!(fs::read_to_string(&next).unwrap(), "concurrent backup");
}

#[tokio::test]
async fn custom_backups_cannot_use_the_target_or_overwrite_files() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.bin");
    let absent = Snapshot::read(&path).await.unwrap();
    let alias = dir.path().join(".").join("config.bin");
    assert_eq!(absent.create_backup_at(&alias).await, Err(Error::Backup));
    assert!(!path.exists());
    fs::write(&path, "# private fixture").unwrap();
    let snapshot = Snapshot::read(&path).await.unwrap();
    let custom = dir.path().join("backup with spaces.bin");
    snapshot.create_backup_at(&custom).await.unwrap();
    assert_eq!(fs::read_to_string(&custom).unwrap(), "# private fixture");
    #[cfg(unix)]
    assert_eq!(
        fs::metadata(&custom).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert!(!snapshot.default_backup_path().await.unwrap().exists());
    assert_eq!(snapshot.create_backup_at(&custom).await, Err(Error::Backup));
    assert_eq!(fs::read_to_string(&custom).unwrap(), "# private fixture");
    let hardlink = dir.path().join("hardlink.bak");
    fs::hard_link(&path, &hardlink).unwrap();
    assert_eq!(
        snapshot.create_backup_at(&hardlink).await,
        Err(Error::Backup)
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), "# private fixture");
}

#[cfg(unix)]
#[tokio::test]
async fn refuses_symlink_targets() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.bin");
    fs::write(&path, b"original bytes").unwrap();
    let link = dir.path().join("link.bin");
    symlink(&path, &link).unwrap();
    assert!(matches!(Snapshot::backup(&link).await, Err(Error::Backup)));
    assert!(!link.with_extension("bin.~1~").exists());
}

#[cfg(unix)]
#[tokio::test]
async fn temporary_write_failure_leaves_original_and_backup_intact() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.bin");
    fs::write(&path, "# original").unwrap();
    let snapshot = Snapshot::backup(&path).await.unwrap();
    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o500)).unwrap();
    // Root can bypass directory permissions; in that environment this failure
    // cannot be induced with permissions, so restore and return.
    if tempfile::NamedTempFile::new_in(dir.path()).is_ok() {
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
        return;
    }
    let result = snapshot.replace(b"new bytes\0\xff").await;
    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(result, Err(Error::Write));
    assert_eq!(fs::read_to_string(&path).unwrap(), "# original");
    assert_eq!(
        fs::read_to_string(path.with_extension("bin.~1~")).unwrap(),
        "# original"
    );
}

#[tokio::test]
async fn arbitrary_bytes_round_trip_and_identical_writes_are_skipped() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("settings.bin");
    let contents = b"private bytes\0\xff";
    assert!(
        Snapshot::read(&path)
            .await
            .unwrap()
            .replace(contents)
            .await
            .unwrap()
    );
    let snapshot = Snapshot::backup(&path).await.unwrap();
    assert_eq!(snapshot.contents(), contents);
    assert_eq!(
        fs::read(directory.path().join("settings.bin.~1~")).unwrap(),
        contents
    );
    let metadata = fs::metadata(&path).unwrap();
    #[cfg(unix)]
    assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
    assert!(!snapshot.replace(contents).await.unwrap());
    assert_eq!(
        fs::metadata(&path).unwrap().modified().unwrap(),
        metadata.modified().unwrap()
    );
    let snapshot = Snapshot::read(&path).await.unwrap();
    fs::write(&path, b"external edit").unwrap();
    // Stale detection must run even when the desired bytes match the old snapshot.
    assert_eq!(snapshot.replace(contents).await, Err(Error::Changed));
    assert_eq!(fs::read(&path).unwrap(), b"external edit");
}

#[tokio::test]
async fn empty_write_creates_an_absent_file_but_preserves_an_existing_empty_file() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("settings.bin");
    let snapshot = Snapshot::backup(&path).await.unwrap();
    assert_eq!(
        fs::read(directory.path().join("settings.bin.~1~")).unwrap(),
        b""
    );
    assert!(snapshot.contents().is_empty());
    assert!(snapshot.replace(b"").await.unwrap());
    assert!(path.is_file());
    let metadata = fs::metadata(&path).unwrap();
    assert!(
        !Snapshot::read(&path)
            .await
            .unwrap()
            .replace(b"")
            .await
            .unwrap()
    );
    assert_eq!(
        fs::metadata(&path).unwrap().modified().unwrap(),
        metadata.modified().unwrap()
    );
}
