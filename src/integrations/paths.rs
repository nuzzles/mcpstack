//! Resolve literal filesystem paths without inspecting or executing servers.
use std::ffi::OsStr;
use std::path::Path;

use typed_path::Utf8WindowsPath;

use crate::error::AppError;
use crate::schema::v1::{ConfigValue, PathResolution, Transport};
use crate::schema::{Server, StackV1};

pub(super) async fn resolve(stack: &mut StackV1, file: Option<&Path>) -> Result<(), AppError> {
    let base = if let Some(file) = file {
        let file = std::path::absolute(file).map_err(AppError::StackRead)?;
        // A symlinked stack uses the directory of the link. Server paths need not exist.
        Some(
            tokio::fs::canonicalize(file.parent().ok_or(AppError::StackPath)?)
                .await
                .map_err(AppError::StackRead)?,
        )
    } else {
        None
    };
    for server in stack.servers.values_mut() {
        match server {
            Server::Portable {
                transport:
                    Transport::Stdio {
                        command,
                        cwd,
                        path_resolution: PathResolution::Stack,
                        ..
                    },
                ..
            } => {
                resolve_value(command, base.as_deref(), true)?;
                if let Some(cwd) = cwd {
                    resolve_value(cwd, base.as_deref(), false)?;
                }
            }
            Server::Configuration { config } => {
                for (key, executable) in [("command", true), ("cwd", false)] {
                    if let Some(ConfigValue::String(value)) = config.get_mut(key) {
                        resolve_value(value, base.as_deref(), executable)?;
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn resolve_value(
    value: &mut String,
    base: Option<&Path>,
    executable: bool,
) -> Result<(), AppError> {
    if value.trim().is_empty() || value.contains("${") {
        return Ok(());
    }
    let path = Path::new(value.as_str());
    // Parse Windows syntax on every host, including UNC and verbatim paths.
    let windows = Utf8WindowsPath::new(value.as_str());
    if path.has_root() || windows.is_absolute() {
        return Ok(());
    }
    // Drive-relative Windows paths depend on per-drive process state, so they
    // cannot be resolved relative to a stack directory deterministically.
    if cfg!(windows) && windows.components().has_prefix() {
        return Err(AppError::StackPath);
    }
    if executable && path.file_name() == Some(OsStr::new(value.as_str())) {
        return Ok(());
    }
    let base = base.ok_or(AppError::StackRemotePath)?;
    let resolved = std::path::absolute(base.join(path)).map_err(AppError::StackRead)?;
    *value = resolved.to_str().ok_or(AppError::StackPath)?.to_owned();
    Ok(())
}
