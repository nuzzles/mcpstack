//! Private Windows files: explicit user-only DACL at creation, before any data.
use std::ffi::c_void;
use std::fs::File;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::{Component, Path};
use std::ptr::{addr_of, null_mut};

use windows_sys::Win32::Foundation::{
    GENERIC_READ, GENERIC_WRITE, INVALID_HANDLE_VALUE, LocalFree,
};
use windows_sys::Win32::Globalization::{CSTR_EQUAL, CompareStringOrdinal};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, GetSecurityInfo,
    SE_FILE_OBJECT,
};
use windows_sys::Win32::Security::{
    ACCESS_ALLOWED_ACE, DACL_SECURITY_INFORMATION, EqualSid, GetAce, GetSecurityDescriptorControl,
    GetTokenInformation, SE_DACL_PROTECTED, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER,
    TokenUser,
};
use windows_sys::Win32::Storage::FileSystem::{
    CREATE_NEW, CreateFileW, FILE_ALL_ACCESS, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_DELETE,
    FILE_SHARE_READ, GetVolumeInformationByHandleW,
};
use windows_sys::Win32::System::SystemServices::{ACCESS_ALLOWED_ACE_TYPE, FILE_PERSISTENT_ACLS};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

struct LocalAllocation(*mut c_void);

impl Drop for LocalAllocation {
    fn drop(&mut self) {
        // SAFETY: All instances wrap an allocation returned by an API that
        // explicitly requires LocalFree; no references survive this owner.
        unsafe {
            LocalFree(self.0);
        }
    }
}

/// Reject alternate streams, device namespaces, and DOS filename aliases. These
/// are not standalone config files and can bypass create-new/path comparisons.
pub(super) fn validate_path(path: &Path) -> io::Result<()> {
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => {
                use std::path::Prefix;
                if !matches!(
                    prefix.kind(),
                    Prefix::Disk(_)
                        | Prefix::UNC(_, _)
                        | Prefix::VerbatimDisk(_)
                        | Prefix::VerbatimUNC(_, _)
                ) {
                    return Err(io::Error::other("unsupported file namespace"));
                }
            }
            Component::Normal(name) => {
                let text = name.to_string_lossy();
                let stem = text
                    .split('.')
                    .next()
                    .unwrap_or_default()
                    .trim_end_matches(' ')
                    .to_uppercase();
                if text.contains([':', '\0'])
                    || text.ends_with(['.', ' '])
                    || matches!(
                        stem.as_str(),
                        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
                    )
                    || ["COM", "LPT"].iter().any(|prefix| {
                        stem.strip_prefix(prefix).is_some_and(|suffix| {
                            matches!(
                                suffix,
                                "1" | "2"
                                    | "3"
                                    | "4"
                                    | "5"
                                    | "6"
                                    | "7"
                                    | "8"
                                    | "9"
                                    | "¹"
                                    | "²"
                                    | "³"
                            )
                        })
                    })
                {
                    return Err(io::Error::other("not a standalone file path"));
                }
            }
            _ => {}
        }
    }
    Ok(())
}

pub(super) fn same_path(left: &Path, right: &Path) -> bool {
    let left: Vec<_> = left.as_os_str().encode_wide().collect();
    let right: Vec<_> = right.as_os_str().encode_wide().collect();
    let (Ok(left_len), Ok(right_len)) = (i32::try_from(left.len()), i32::try_from(right.len()))
    else {
        return true; // Fail closed for an unrepresentable path length.
    };
    // SAFETY: Both buffers live through the call and lengths count UTF-16 units.
    unsafe {
        CompareStringOrdinal(left.as_ptr(), left_len, right.as_ptr(), right_len, 1) == CSTR_EQUAL
    }
}

// usize storage gives TOKEN_USER the alignment required by the Windows API.
fn current_user() -> io::Result<Vec<usize>> {
    unsafe {
        let mut token = null_mut();
        // SAFETY: The output handle is initialized by OpenProcessToken and owned below.
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return Err(io::Error::last_os_error());
        }
        let token = OwnedHandle::from_raw_handle(token);
        let mut size = 0;
        GetTokenInformation(token.as_raw_handle(), TokenUser, null_mut(), 0, &mut size);
        if size < size_of::<TOKEN_USER>() as u32 {
            return Err(io::Error::last_os_error());
        }
        let mut data = vec![0usize; (size as usize).div_ceil(size_of::<usize>())];
        if GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            data.as_mut_ptr().cast(),
            size,
            &mut size,
        ) == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(data)
    }
}

pub(super) fn create_private(path: &Path) -> io::Result<File> {
    validate_path(path)?;
    let user = current_user()?;
    // SAFETY: TOKEN_USER and its SID reside in the aligned buffer, which stays
    // alive until the security descriptor and file have been constructed.
    unsafe {
        let sid = (*user.as_ptr().cast::<TOKEN_USER>()).User.Sid;
        let mut sid_text = null_mut();
        if ConvertSidToStringSidW(sid, &mut sid_text) == 0 {
            return Err(io::Error::last_os_error());
        }
        let _sid_text = LocalAllocation(sid_text.cast());
        let mut length = 0;
        while *sid_text.add(length) != 0 {
            length += 1;
        }
        let sid_text = String::from_utf16(std::slice::from_raw_parts(sid_text, length))
            .map_err(io::Error::other)?;
        // Set owner explicitly, and protect the DACL against inherited entries.
        let sddl: Vec<_> = format!("O:{sid_text}D:P(A;;FA;;;{sid_text})")
            .encode_utf16()
            .chain([0])
            .collect();
        let mut descriptor = null_mut();
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            1,
            &mut descriptor,
            null_mut(),
        ) == 0
        {
            return Err(io::Error::last_os_error());
        }
        let _descriptor = LocalAllocation(descriptor);
        let attributes = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor,
            bInheritHandle: 0,
        };
        let path: Vec<_> = path.as_os_str().encode_wide().chain([0]).collect();
        let handle = CreateFileW(
            path.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_DELETE,
            &attributes,
            CREATE_NEW,
            FILE_ATTRIBUTE_NORMAL,
            null_mut(),
        );
        if handle == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        let file = File::from_raw_handle(handle);
        // Filesystems such as FAT may silently ignore security descriptors.
        // Refuse them before any credential data is written.
        let mut flags = 0;
        if GetVolumeInformationByHandleW(
            handle,
            null_mut(),
            0,
            null_mut(),
            null_mut(),
            &mut flags,
            null_mut(),
            0,
        ) == 0
        {
            return Err(io::Error::last_os_error());
        }
        if flags & FILE_PERSISTENT_ACLS == 0 {
            return Err(io::Error::other(
                "filesystem cannot enforce private permissions",
            ));
        }
        verify_private(&file)?;
        Ok(file)
    }
}

/// Read back the applied DACL instead of assuming the filesystem honored it.
pub(super) fn verify_private(file: &File) -> io::Result<()> {
    let user = current_user()?;
    // SAFETY: GetSecurityInfo owns the returned descriptor allocation. DACL and
    // ACE pointers refer into it and are used only while the allocation is alive.
    unsafe {
        let mut descriptor = null_mut();
        let mut dacl = null_mut();
        let code = GetSecurityInfo(
            file.as_raw_handle(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            null_mut(),
            null_mut(),
            &mut dacl,
            null_mut(),
            &mut descriptor,
        );
        if code != 0 {
            return Err(io::Error::from_raw_os_error(code as i32));
        }
        let _descriptor = LocalAllocation(descriptor);
        let mut control = 0;
        let mut revision = 0;
        if GetSecurityDescriptorControl(descriptor, &mut control, &mut revision) == 0 {
            return Err(io::Error::last_os_error());
        }
        if control & SE_DACL_PROTECTED == 0 || dacl.is_null() || (*dacl).AceCount != 1 {
            return Err(io::Error::other("file permissions are not private"));
        }
        let mut ace = null_mut();
        if GetAce(dacl, 0, &mut ace) == 0 {
            return Err(io::Error::last_os_error());
        }
        let ace = &*ace.cast::<ACCESS_ALLOWED_ACE>();
        let sid = (*user.as_ptr().cast::<TOKEN_USER>()).User.Sid;
        if u32::from(ace.Header.AceType) != ACCESS_ALLOWED_ACE_TYPE
            || ace.Header.AceFlags != 0
            || ace.Mask != FILE_ALL_ACCESS
            || EqualSid(addr_of!(ace.SidStart).cast_mut().cast(), sid) == 0
        {
            return Err(io::Error::other("file permissions are not user-only"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::{FileError, Snapshot};
    use super::*;
    use std::collections::BTreeMap;
    use std::fs;
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Security::{PROTECTED_DACL_SECURITY_INFORMATION, SetFileSecurityW};

    fn allow_inherited_access(directory: &Path) {
        // Synthetic fixtures only: prove that files do not inherit Everyone access.
        let sddl: Vec<_> = "D:P(A;OICI;FA;;;WD)".encode_utf16().chain([0]).collect();
        let path: Vec<_> = directory.as_os_str().encode_wide().chain([0]).collect();
        // SAFETY: Both strings are terminated; the descriptor lives through the call.
        unsafe {
            let mut descriptor = null_mut();
            assert_ne!(
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    sddl.as_ptr(),
                    1,
                    &mut descriptor,
                    null_mut(),
                ),
                0
            );
            let _descriptor = LocalAllocation(descriptor);
            assert_ne!(
                SetFileSecurityW(
                    path.as_ptr(),
                    DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                    descriptor,
                ),
                0
            );
        }
    }

    fn definitions() -> BTreeMap<String, toml::Table> {
        BTreeMap::from([(
            "added".into(),
            toml::from_str("command='tool'\nenv={API_KEY='fixture-secret'}").unwrap(),
        )])
    }

    #[test]
    fn private_creation_and_replacement_keep_user_only_acls() {
        let directory = tempfile::tempdir().unwrap();
        allow_inherited_access(directory.path());
        // Inspect the temporary file while empty, before any credential write.
        let temporary = tempfile::Builder::new()
            .make_in(directory.path(), create_private)
            .unwrap();
        assert_eq!(temporary.as_file().metadata().unwrap().len(), 0);
        verify_private(temporary.as_file()).unwrap();
        drop(temporary);
        let path = directory.path().join("配置 with spaces.toml");
        fs::write(&path, "# keep\nmodel='existing'\n").unwrap();
        assert!(verify_private(&File::open(&path).unwrap()).is_err());
        let original = fs::read(&path).unwrap();
        let snapshot = Snapshot::backup(&path).unwrap();
        let backup = directory.path().join("配置 with spaces.toml.~1~");
        verify_private(&File::open(&backup).unwrap()).unwrap();
        assert_eq!(fs::read(&backup).unwrap(), original);
        assert_eq!(snapshot.apply(&definitions()).unwrap(), 1);
        verify_private(&File::open(&path).unwrap()).unwrap();
        let written = fs::read(&path).unwrap();
        let modified = fs::metadata(&path).unwrap().modified().unwrap();
        assert_eq!(
            Snapshot::backup(&path)
                .unwrap()
                .apply(&definitions())
                .unwrap(),
            0
        );
        assert_eq!(fs::read(&path).unwrap(), written);
        assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), modified);
        verify_private(&File::open(directory.path().join("配置 with spaces.toml.~2~")).unwrap())
            .unwrap();
    }

    #[test]
    fn absent_config_custom_backups_and_stale_snapshots() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("Config.toml");
        let snapshot = Snapshot::read(&path).unwrap();
        assert_eq!(
            snapshot.create_backup_at(&directory.path().join("config.TOML")),
            Err(FileError::Backup)
        );
        assert!(!path.exists());
        // Backup can also be skipped; the config must still be private.
        assert_eq!(snapshot.apply(&definitions()).unwrap(), 1);
        verify_private(&File::open(&path).unwrap()).unwrap();
        let snapshot = Snapshot::read(&path).unwrap();
        let custom = directory.path().join("custom.bak");
        snapshot.create_backup_at(&custom).unwrap();
        let bytes = fs::read(&custom).unwrap();
        assert_eq!(snapshot.create_backup_at(&custom), Err(FileError::Backup));
        assert_eq!(fs::read(&custom).unwrap(), bytes);
        verify_private(&File::open(&custom).unwrap()).unwrap();
        let alias = directory.path().join("hardlink.bak");
        fs::hard_link(&path, &alias).unwrap();
        assert_eq!(snapshot.create_backup_at(&alias), Err(FileError::Backup));
        assert_eq!(fs::read(&path).unwrap(), bytes);
        fs::write(&path, "# external edit").unwrap();
        assert_eq!(snapshot.apply(&definitions()), Err(FileError::Changed));
        assert_eq!(fs::read_to_string(&path).unwrap(), "# external edit");
    }

    #[test]
    fn numbered_backups_recognize_case_insensitive_generations() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("Config.toml");
        fs::write(&path, "# original").unwrap();
        fs::write(directory.path().join("config.TOML.~3~"), "# old backup").unwrap();
        let snapshot = Snapshot::read(&path).unwrap();
        let backup = snapshot.default_backup_path().unwrap();
        assert_eq!(backup, directory.path().join("Config.toml.~4~"));
        snapshot.create_backup_at(&backup).unwrap();
        assert_eq!(fs::read_to_string(&backup).unwrap(), "# original");
        verify_private(&File::open(&backup).unwrap()).unwrap();
    }

    #[test]
    fn locked_target_leaves_original_and_backup_intact() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        let original = "# original\n";
        fs::write(&path, original).unwrap();
        let snapshot = Snapshot::backup(&path).unwrap();
        // Readers can open the file, but replacement requires delete sharing.
        let lock = fs::OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .open(&path)
            .unwrap();
        assert_eq!(snapshot.apply(&definitions()), Err(FileError::Write));
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
        assert_eq!(
            fs::read_to_string(directory.path().join("config.toml.~1~")).unwrap(),
            original
        );
        drop(lock);
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 2);
    }

    #[test]
    fn read_only_target_is_preserved_on_replacement_failure() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        fs::write(&path, "# original").unwrap();
        let original_permissions = fs::metadata(&path).unwrap().permissions();
        let mut permissions = original_permissions.clone();
        permissions.set_readonly(true);
        fs::set_permissions(&path, permissions).unwrap();
        let result = Snapshot::backup(&path).unwrap().apply(&definitions());
        // Restore the fixture so that TempDir can remove it, even on assertion failure.
        fs::set_permissions(&path, original_permissions).unwrap();
        assert_eq!(result, Err(FileError::Write));
        assert_eq!(fs::read_to_string(&path).unwrap(), "# original");
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 2);
    }

    #[test]
    fn windows_streams_and_device_aliases_are_rejected() {
        for path in [
            r"C:\config.toml:secret",
            r"C:\config.toml.",
            r"C:\config.toml ",
            r"C:\NUL",
            r"C:\COM1.toml",
            r"C:\NUL .toml",
            r"\\.\PhysicalDrive0",
            r"\\?\GLOBALROOT\Device\HarddiskVolume1\config.toml",
        ] {
            assert!(validate_path(Path::new(path)).is_err(), "{path}");
        }
        for path in [
            r"C:\Users\name\config.toml",
            r"\\?\C:\Users\name\config.toml",
            r"\\server\share\config.toml",
        ] {
            validate_path(Path::new(path)).unwrap();
        }
    }
}
