//! Resolve literal filesystem paths without inspecting or executing servers.
use std::path::{Component, Path};

use crate::error::AppError;
use crate::schema::v1::{ConfigValue, Transport};
use crate::schema::{Server, StackV1};

pub(super) async fn resolve(stack: &mut StackV1, file: &Path) -> Result<(), AppError> {
    let file = std::path::absolute(file).map_err(AppError::StackRead)?;
    // Resolve the containing directory, not the file: a symlinked stack uses
    // the directory of the link. Server paths themselves need not exist.
    let base = tokio::fs::canonicalize(file.parent().ok_or(AppError::StackPath)?)
        .await
        .map_err(AppError::StackRead)?;
    for server in stack.servers.values_mut() {
        match server {
            Server::Portable {
                transport: Transport::Stdio { command, cwd, .. },
                ..
            } => {
                resolve_value(command, &base, true)?;
                if let Some(cwd) = cwd {
                    resolve_value(cwd, &base, false)?;
                }
            }
            Server::Configuration { config } => {
                for (key, executable) in [("command", true), ("cwd", false)] {
                    if let Some(ConfigValue::String(value)) = config.get_mut(key) {
                        resolve_value(value, &base, executable)?;
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn resolve_value(value: &mut String, base: &Path, executable: bool) -> Result<(), AppError> {
    let path = Path::new(value.as_str());
    // Leave absolute paths and client-side interpolation intact. Also recognize
    // Windows absolute paths on other platforms rather than prefixing them.
    let bytes = value.as_bytes();
    let windows_absolute = value.starts_with("\\\\")
        || (bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && matches!(bytes[2], b'/' | b'\\'));
    if value.trim().is_empty() || path.has_root() || windows_absolute || value.contains("${") {
        return Ok(());
    }
    // Drive-relative Windows paths depend on per-drive process state, so they
    // cannot be resolved relative to a stack directory deterministically.
    if matches!(path.components().next(), Some(Component::Prefix(_))) {
        return Err(AppError::StackPath);
    }
    if executable && !value.chars().any(std::path::is_separator) && value != "." && value != ".." {
        return Ok(());
    }
    let resolved = std::path::absolute(base.join(path)).map_err(AppError::StackRead)?;
    *value = resolved.to_str().ok_or(AppError::StackPath)?.to_owned();
    Ok(())
}
