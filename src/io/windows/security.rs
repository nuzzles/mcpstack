//! Native security APIs absent from std::fs. Keep FFI and resource ownership here.
use std::ffi::c_void;
use std::fs::File;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::Path;
use std::ptr::{addr_of, null_mut};

use windows_sys::Win32::Foundation::{
    GENERIC_READ, GENERIC_WRITE, INVALID_HANDLE_VALUE, LocalFree,
};
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
    FILE_SHARE_READ, GetVolumeInformationW, GetVolumePathNameW,
};
use windows_sys::Win32::System::SystemServices::{ACCESS_ALLOWED_ACE_TYPE, FILE_PERSISTENT_ACLS};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

struct LocalAllocation(*mut c_void);

impl Drop for LocalAllocation {
    fn drop(&mut self) {
        // SAFETY: Instances own LocalAlloc allocations returned by Windows APIs.
        unsafe {
            LocalFree(self.0);
        }
    }
}

// The SID points inside this buffer. usize storage provides TOKEN_USER alignment.
struct CurrentUser(Vec<usize>);

impl CurrentUser {
    fn query() -> io::Result<Self> {
        let mut token = null_mut();
        // SAFETY: Windows initializes the output; OwnedHandle closes it exactly once.
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: OpenProcessToken succeeded and transferred ownership of this handle.
        let token = unsafe { OwnedHandle::from_raw_handle(token) };
        let mut size = 0;
        // SAFETY: A null buffer and zero length request the required allocation size.
        unsafe {
            GetTokenInformation(token.as_raw_handle(), TokenUser, null_mut(), 0, &mut size);
        }
        if size < size_of::<TOKEN_USER>() as u32 {
            return Err(io::Error::last_os_error());
        }
        let mut data = vec![0usize; (size as usize).div_ceil(size_of::<usize>())];
        // SAFETY: data is aligned, writable, and at least size bytes long.
        if unsafe {
            GetTokenInformation(
                token.as_raw_handle(),
                TokenUser,
                data.as_mut_ptr().cast(),
                size,
                &mut size,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(Self(data))
    }

    fn sid(&self) -> *mut c_void {
        // SAFETY: query initialized TOKEN_USER and the SID in the owned buffer.
        unsafe { (*self.0.as_ptr().cast::<TOKEN_USER>()).User.Sid }
    }

    fn sid_string(&self) -> io::Result<String> {
        let mut text = null_mut();
        // SAFETY: self owns the SID; the output is a LocalFree-owned, terminated string.
        if unsafe { ConvertSidToStringSidW(self.sid(), &mut text) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let _text = LocalAllocation(text.cast());
        // SAFETY: The successful conversion guarantees a UTF-16 string ending in NUL.
        unsafe {
            let mut length = 0;
            while *text.add(length) != 0 {
                length += 1;
            }
            String::from_utf16(std::slice::from_raw_parts(text, length)).map_err(io::Error::other)
        }
    }
}

pub(super) struct SecurityDescriptor(LocalAllocation);

impl SecurityDescriptor {
    pub(super) fn from_sddl(text: &str) -> io::Result<Self> {
        let text: Vec<_> = text.encode_utf16().chain([0]).collect();
        let mut descriptor = null_mut();
        // SAFETY: text is terminated and lives through the call; Windows allocates the output.
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                text.as_ptr(),
                1,
                &mut descriptor,
                null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(Self(LocalAllocation(descriptor)))
    }

    pub(super) fn as_ptr(&self) -> *mut c_void {
        self.0.0
    }

    fn for_user(user: &CurrentUser) -> io::Result<Self> {
        let sid = user.sid_string()?;
        // Explicit owner; protected DACL; one full-access ACE for that user.
        Self::from_sddl(&format!("O:{sid}D:P(A;;FA;;;{sid})"))
    }
}

/// Create an empty private file, checking both filesystem support and actual ACLs
/// before the caller can write bytes. No parsing or client policy belongs here.
pub(in crate::io) fn create_private(path: &Path) -> io::Result<File> {
    let path = super::normalize_path(path)?;
    let path: Vec<_> = path.as_os_str().encode_wide().chain([0]).collect();
    let user = CurrentUser::query()?;
    let descriptor = SecurityDescriptor::for_user(&user)?;
    let file = create_new(&path, &descriptor)?;
    require_persistent_acls(&path)?;
    verify_user_dacl(&file, &user)?;
    Ok(file)
}

fn create_new(path: &[u16], descriptor: &SecurityDescriptor) -> io::Result<File> {
    let attributes = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor.as_ptr(),
        bInheritHandle: 0,
    };
    // SAFETY: The caller supplies a terminated path. The descriptor is owned and
    // lives through this call; a successful handle is transferred into File.
    let handle = unsafe {
        CreateFileW(
            path.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_DELETE,
            &attributes,
            CREATE_NEW,
            FILE_ATTRIBUTE_NORMAL,
            null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: CreateFileW returned a new valid owned handle.
    Ok(unsafe { File::from_raw_handle(handle) })
}

fn require_persistent_acls(path: &[u16]) -> io::Result<()> {
    // Use volume paths so the capability query works over SMB and mapped drives.
    let mut root = vec![0u16; 32768];
    // SAFETY: path is terminated and root is a writable buffer of the stated size.
    if unsafe { GetVolumePathNameW(path.as_ptr(), root.as_mut_ptr(), root.len() as u32) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let mut flags = 0;
    // SAFETY: Windows initialized root with a terminated path; flags is writable.
    if unsafe {
        GetVolumeInformationW(
            root.as_ptr(),
            null_mut(),
            0,
            null_mut(),
            null_mut(),
            &mut flags,
            null_mut(),
            0,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    if flags & FILE_PERSISTENT_ACLS == 0 {
        return Err(io::Error::other(
            "filesystem cannot enforce private permissions",
        ));
    }
    Ok(())
}

#[cfg(test)]
pub(super) fn verify_private(file: &File) -> io::Result<()> {
    verify_user_dacl(file, &CurrentUser::query()?)
}

/// Read back the applied DACL instead of assuming the filesystem honored it.
fn verify_user_dacl(file: &File, user: &CurrentUser) -> io::Result<()> {
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
        let sid = user.sid();
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
