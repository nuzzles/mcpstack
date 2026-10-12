//! Documented local server configuration, separate from Desktop connectors and extensions.
//! https://modelcontextprotocol.io/docs/develop/connect-local-servers
use std::path::PathBuf;

use serde_json::Value;

use super::Error;

pub(super) fn path(config: Option<PathBuf>) -> Result<PathBuf, Error> {
    if let Some(path) = config {
        return Ok(path);
    }
    #[cfg(target_os = "macos")]
    if let Some(dirs) = directories::BaseDirs::new() {
        return Ok(dirs
            .home_dir()
            .join("Library/Application Support/Claude/claude_desktop_config.json"));
    }
    #[cfg(windows)]
    {
        let roaming = std::env::var_os("APPDATA")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .or_else(|| directories::BaseDirs::new().map(|dirs| dirs.config_dir().to_owned()));
        if let Some(roaming) = roaming.filter(|path| path.is_absolute()) {
            return Ok(roaming.join("Claude/claude_desktop_config.json"));
        }
    }
    // No undocumented default on other platforms. Explicit configs are portable.
    Err(Error::DesktopPath)
}

pub(super) fn validate(definition: &Value) -> Result<(), Error> {
    let fields = definition.as_object().ok_or(Error::Config)?;
    if fields
        .keys()
        .any(|key| !matches!(key.as_str(), "type" | "command" | "args" | "env"))
        || fields
            .get("type")
            .is_some_and(|kind| kind.as_str() != Some("stdio"))
    {
        return Err(Error::DesktopConfig);
    }
    Ok(())
}
