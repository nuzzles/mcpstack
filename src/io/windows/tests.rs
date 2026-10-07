use super::super::{Error, Snapshot};
use super::security::{SecurityDescriptor, verify_private};
use super::*;
use std::fs::{self, File};
use std::os::windows::fs::OpenOptionsExt;
use windows_sys::Win32::Security::{
    DACL_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION, SetFileSecurityW,
};
use windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ;

fn allow_inherited_access(directory: &Path) {
    // Synthetic fixtures only: prove that files do not inherit Everyone access.
    let descriptor = SecurityDescriptor::from_sddl("D:P(A;OICI;FA;;;WD)").unwrap();
    let path: Vec<_> = directory.as_os_str().encode_wide().chain([0]).collect();
    // SAFETY: path is terminated and the descriptor remains alive through the call.
    assert_ne!(
        unsafe {
            SetFileSecurityW(
                path.as_ptr(),
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                descriptor.as_ptr(),
            )
        },
        0
    );
}

fn contents() -> &'static [u8] {
    b"private fixture bytes\0\xff"
}

async fn check_long_path_writes(root: &Path) {
    let mut parent = root.to_path_buf();
    for _ in 0..6 {
        parent.push("long directory with spaces and unicode 配置".repeat(2));
    }
    fs::create_dir_all(&parent).unwrap();
    let path = parent.join("config.bin");
    assert!(path.as_os_str().encode_wide().count() > 260);
    // Creation uses persist_noclobber; replacement uses persist.
    let snapshot = Snapshot::read(&path).await.unwrap();
    let custom = parent.join("custom backup.bin");
    snapshot.create_backup_at(&custom).await.unwrap();
    assert!(snapshot.replace(contents()).await.unwrap());
    verify_private(&File::open(&path).unwrap()).unwrap();
    verify_private(&File::open(&custom).unwrap()).unwrap();
    let original = fs::read(&path).unwrap();
    let snapshot = Snapshot::backup(&path).await.unwrap();
    let changed = b"different private bytes\0\xfe";
    assert!(snapshot.replace(changed).await.unwrap());
    verify_private(&File::open(&path).unwrap()).unwrap();
    let backup = parent.join("config.bin.~1~");
    assert_eq!(fs::read(&backup).unwrap(), original);
    verify_private(&File::open(&backup).unwrap()).unwrap();
    assert!(
        !Snapshot::read(&path)
            .await
            .unwrap()
            .replace(changed)
            .await
            .unwrap()
    );
    assert_eq!(fs::read_dir(&parent).unwrap().count(), 3);
}

#[tokio::test]
async fn long_paths_support_creation_backups_and_replacement() {
    let directory = tempfile::tempdir().unwrap();
    // Deliberately pass an ordinary path even if TEMP was already verbatim.
    let path = normalize_path(directory.path()).unwrap();
    let wide: Vec<_> = path.as_os_str().encode_wide().collect();
    assert!(
        matches!(path.components().next(), Some(Component::Prefix(p)) if matches!(p.kind(), Prefix::VerbatimDisk(_)))
    );
    check_long_path_writes(Path::new(&OsString::from_wide(&wide[4..]))).await;
}

#[tokio::test]
async fn normalized_paths_preserve_relative_and_unc_semantics() {
    for path in [r"C:/folder/child/../config.bin", r"C:\folder\config.bin"] {
        assert_eq!(
            normalize_path(Path::new(path)).unwrap(),
            Path::new(r"\\?\C:\folder\config.bin")
        );
    }
    assert_eq!(
        normalize_path(Path::new(r"\\server\share\child\..\config.bin")).unwrap(),
        Path::new(r"\\?\UNC\server\share\config.bin")
    );
    let relative = normalize_path(Path::new("config.bin")).unwrap();
    assert_eq!(
        relative,
        normalize_path(&std::env::current_dir().unwrap().join("config.bin")).unwrap()
    );
    assert_eq!(normalize_path(&relative).unwrap(), relative);
    for path in [r"C:\config.bin:secret", r"C:\NUL", r"\\.\PhysicalDrive0"] {
        assert!(normalize_path(Path::new(path)).is_err());
    }
}

#[tokio::test]
#[ignore = "requires the temporary SMB share provisioned by test-smb.ps1"]
async fn smb_writes_support_unc_and_mapped_drive_paths() {
    for variable in ["MCPSTACK_TEST_SMB_UNC", "MCPSTACK_TEST_SMB_DRIVE"] {
        let root = std::env::var_os(variable).expect("SMB fixture must be configured");
        let directory = tempfile::tempdir_in(root).unwrap();
        check_long_path_writes(directory.path()).await;
    }
}

#[tokio::test]
async fn private_creation_and_replacement_keep_user_only_acls() {
    let directory = tempfile::tempdir().unwrap();
    allow_inherited_access(directory.path());
    // Inspect the temporary file while empty, before any credential write.
    let temporary = tempfile::Builder::new()
        .make_in(directory.path(), create_private)
        .unwrap();
    assert_eq!(temporary.as_file().metadata().unwrap().len(), 0);
    verify_private(temporary.as_file()).unwrap();
    drop(temporary);
    let path = directory.path().join("配置 with spaces.bin");
    fs::write(&path, "original private bytes\0").unwrap();
    assert!(verify_private(&File::open(&path).unwrap()).is_err());
    let original = fs::read(&path).unwrap();
    let snapshot = Snapshot::backup(&path).await.unwrap();
    let backup = directory.path().join("配置 with spaces.bin.~1~");
    verify_private(&File::open(&backup).unwrap()).unwrap();
    assert_eq!(fs::read(&backup).unwrap(), original);
    assert!(snapshot.replace(contents()).await.unwrap());
    verify_private(&File::open(&path).unwrap()).unwrap();
    let written = fs::read(&path).unwrap();
    let modified = fs::metadata(&path).unwrap().modified().unwrap();
    assert!(
        !Snapshot::backup(&path)
            .await
            .unwrap()
            .replace(contents())
            .await
            .unwrap()
    );
    assert_eq!(fs::read(&path).unwrap(), written);
    assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), modified);
    verify_private(&File::open(directory.path().join("配置 with spaces.bin.~2~")).unwrap())
        .unwrap();
}

#[tokio::test]
async fn absent_config_custom_backups_and_stale_snapshots() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("Config.bin");
    let snapshot = Snapshot::read(&path).await.unwrap();
    assert_eq!(
        snapshot
            .create_backup_at(&directory.path().join("config.BIN"))
            .await,
        Err(Error::Backup)
    );
    assert!(!path.exists());
    // Backup can also be skipped; the config must still be private.
    assert!(snapshot.replace(contents()).await.unwrap());
    verify_private(&File::open(&path).unwrap()).unwrap();
    let snapshot = Snapshot::read(&path).await.unwrap();
    let custom = directory.path().join("custom.bak");
    snapshot.create_backup_at(&custom).await.unwrap();
    let bytes = fs::read(&custom).unwrap();
    assert_eq!(snapshot.create_backup_at(&custom).await, Err(Error::Backup));
    assert_eq!(fs::read(&custom).unwrap(), bytes);
    verify_private(&File::open(&custom).unwrap()).unwrap();
    let alias = directory.path().join("hardlink.bak");
    fs::hard_link(&path, &alias).unwrap();
    assert_eq!(snapshot.create_backup_at(&alias).await, Err(Error::Backup));
    assert_eq!(fs::read(&path).unwrap(), bytes);
    fs::write(&path, "# external edit").unwrap();
    assert_eq!(snapshot.replace(contents()).await, Err(Error::Changed));
    assert_eq!(fs::read_to_string(&path).unwrap(), "# external edit");
}

#[tokio::test]
async fn numbered_backups_recognize_case_insensitive_generations() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("Config.bin");
    fs::write(&path, "# original").unwrap();
    fs::write(directory.path().join("config.BIN.~3~"), "# old backup").unwrap();
    let snapshot = Snapshot::read(&path).await.unwrap();
    let backup = snapshot.default_backup_path().await.unwrap();
    assert_eq!(backup, directory.path().join("Config.bin.~4~"));
    snapshot.create_backup_at(&backup).await.unwrap();
    assert_eq!(fs::read_to_string(&backup).unwrap(), "# original");
    verify_private(&File::open(&backup).unwrap()).unwrap();
}

#[tokio::test]
async fn locked_target_leaves_original_and_backup_intact() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("config.bin");
    let original = "# original\n";
    fs::write(&path, original).unwrap();
    let snapshot = Snapshot::backup(&path).await.unwrap();
    // Readers can open the file, but replacement requires delete sharing.
    let lock = fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .open(&path)
        .unwrap();
    assert_eq!(snapshot.replace(contents()).await, Err(Error::Write));
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    assert_eq!(
        fs::read_to_string(directory.path().join("config.bin.~1~")).unwrap(),
        original
    );
    drop(lock);
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 2);
}

#[tokio::test]
async fn read_only_target_is_preserved_on_replacement_failure() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("config.bin");
    fs::write(&path, "# original").unwrap();
    let original_permissions = fs::metadata(&path).unwrap().permissions();
    let mut permissions = original_permissions.clone();
    permissions.set_readonly(true);
    fs::set_permissions(&path, permissions).unwrap();
    let result = Snapshot::backup(&path)
        .await
        .unwrap()
        .replace(contents())
        .await;
    // Restore the fixture so that TempDir can remove it, even on assertion failure.
    fs::set_permissions(&path, original_permissions).unwrap();
    assert_eq!(result, Err(Error::Write));
    assert_eq!(fs::read_to_string(&path).unwrap(), "# original");
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 2);
}

#[tokio::test]
async fn windows_streams_and_device_aliases_are_rejected() {
    for path in [
        r"C:\config.bin:secret",
        r"C:\config.bin.",
        r"C:\config.bin ",
        r"C:\NUL",
        r"C:\COM1.bin",
        r"C:\NUL .bin",
        r"\\.\PhysicalDrive0",
        r"\\?\GLOBALROOT\Device\HarddiskVolume1\config.bin",
    ] {
        assert!(validate_path(Path::new(path)).is_err(), "{path}");
    }
    for path in [
        r"C:\Users\name\config.bin",
        r"\\?\C:\Users\name\config.bin",
        r"\\server\share\config.bin",
    ] {
        validate_path(Path::new(path)).unwrap();
    }
}
