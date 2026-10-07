//! Windows path handling used by shared I/O.
use std::ffi::OsString;
use std::io;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Component, Path, PathBuf, Prefix};

use windows_sys::Win32::Globalization::{CSTR_EQUAL, CompareStringOrdinal};

/// Reject alternate streams, device namespaces, and DOS filename aliases. These
/// are not standalone config files and can bypass create-new/path comparisons.
pub(super) fn validate_path(path: &Path) -> io::Result<()> {
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => {
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

/// Normalize separators and relative components before adding a verbatim prefix.
/// All path-based Win32 calls (including tempfile's rename) need this form to
/// support long paths without machine-wide policy or application manifest changes.
pub(super) fn normalize_path(path: &Path) -> io::Result<PathBuf> {
    validate_path(path)?;
    let absolute = std::path::absolute(path)?;
    let Some(Component::Prefix(prefix)) = absolute.components().next() else {
        return Err(io::Error::other("expected an absolute file path"));
    };
    let (prefix, skip) = match prefix.kind() {
        Prefix::VerbatimDisk(_) | Prefix::VerbatimUNC(_, _) => return Ok(absolute),
        Prefix::Disk(_) => (r"\\?\", 0),
        Prefix::UNC(_, _) => (r"\\?\UNC\", 2),
        _ => return Err(io::Error::other("unsupported file namespace")),
    };
    let wide: Vec<_> = prefix
        .encode_utf16()
        .chain(absolute.as_os_str().encode_wide().skip(skip))
        .collect();
    Ok(OsString::from_wide(&wide).into())
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

#[cfg(test)]
mod tests;
