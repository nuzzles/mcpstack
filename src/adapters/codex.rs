use std::process::{Command, Stdio};

use semver::Version;
use thiserror::Error;

use crate::exporters::codex::{ExportError, export, export_with_decisions};
use crate::schema::StackV1;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum AdapterError {
    #[error(
        "Unable to detect Codex version. Ensure codex is installed and codex --version succeeds."
    )]
    Detection,
    #[error("Codex 1.0.0 and later, including prereleases, require an explicit supported adapter.")]
    UnsupportedMajor,
    #[error("Import supports only the checked stable Codex version 0.160.0.")]
    UnsupportedImportVersion,
}

/// Select an adapter without reading or modifying client configuration.
pub fn detect() -> Result<CodexAdapter, AdapterError> {
    let version = detect_version()?;
    CodexAdapter::select(&version)
}

fn detect_version() -> Result<Version, AdapterError> {
    let output = codex_command()?
        .arg("--version")
        .stdin(Stdio::null())
        .output()
        .map_err(|_| AdapterError::Detection)?;
    if !output.status.success() {
        return Err(AdapterError::Detection);
    }
    parse_version(&output.stdout)
}

#[cfg(not(windows))]
fn codex_command() -> Result<Command, AdapterError> {
    Ok(Command::new("codex"))
}

#[cfg(windows)]
fn codex_command() -> Result<Command, AdapterError> {
    let path = std::env::var_os("PATH").ok_or(AdapterError::Detection)?;
    let launcher = windows_launcher(&path).ok_or(AdapterError::Detection)?;
    // Rust's Command handles an explicitly resolved .cmd via cmd.exe. Let it
    // quote the path and arguments rather than building a shell command string.
    Ok(Command::new(launcher))
}

#[cfg(any(windows, test))]
fn windows_launcher(path: &std::ffi::OsStr) -> Option<std::path::PathBuf> {
    for directory in std::env::split_paths(path) {
        // Match PATH order, preferring a native executable within each directory.
        for name in ["codex.exe", "codex.cmd"] {
            let candidate = directory.join(name);
            if candidate.is_file() {
                return std::path::absolute(candidate).ok();
            }
        }
    }
    None
}

fn parse_version(output: &[u8]) -> Result<Version, AdapterError> {
    let output = std::str::from_utf8(output).map_err(|_| AdapterError::Detection)?;
    let mut words = output.split_whitespace();
    if words.next() != Some("codex-cli") {
        return Err(AdapterError::Detection);
    }
    let version = words.next().ok_or(AdapterError::Detection)?;
    if words.next().is_some() {
        return Err(AdapterError::Detection);
    }
    Version::parse(version).map_err(|_| AdapterError::Detection)
}

/// An adapter validated for writes, constructed only after exact-version detection.
pub struct CodexImportAdapter(());

#[allow(dead_code, reason = "foundation for the stacked import CLI PR")]
pub fn detect_import() -> Result<CodexImportAdapter, AdapterError> {
    CodexImportAdapter::select(&detect_version()?)
}

impl CodexImportAdapter {
    fn select(version: &Version) -> Result<Self, AdapterError> {
        if version != &CURRENT_STABLE {
            return Err(AdapterError::UnsupportedImportVersion);
        }
        Ok(Self(()))
    }

    #[allow(dead_code, reason = "foundation for the stacked import CLI PR")]
    pub fn prepare(
        &self,
        stack: &StackV1,
        lookup: impl FnMut(&str) -> Option<String>,
    ) -> Result<std::collections::BTreeMap<String, toml::Table>, crate::importers::codex::ImportError>
    {
        crate::importers::codex::prepare(stack, lookup)
    }
}

/// Latest stable release checked against the native MCP TOML layout.
/// https://github.com/openai/codex/releases/tag/rust-v0.160.0
pub const CURRENT_STABLE: Version = Version::new(0, 160, 0);

#[derive(Debug, PartialEq, Eq)]
pub enum CodexAdapter {
    Pre1,
    Newer { version: Version },
}

impl CodexAdapter {
    pub fn select(version: &Version) -> Result<Self, AdapterError> {
        if version.major >= 1 {
            return Err(AdapterError::UnsupportedMajor);
        }
        // Historical stdio/HTTP/auth field changes stay inside mcp_servers;
        // native export preserves their spellings and values without translation.
        // https://github.com/openai/codex/commit/3a1be084f911
        // https://github.com/openai/codex/commit/a43ae86b6c07
        if version.cmp_precedence(&CURRENT_STABLE).is_gt() {
            Ok(Self::Newer {
                version: version.clone(),
            })
        } else {
            Ok(Self::Pre1)
        }
    }

    pub fn warning(&self) -> Option<String> {
        match self {
            Self::Pre1 => None,
            Self::Newer { version } => Some(format!(
                "Codex {version} is newer than the checked stable release {CURRENT_STABLE}; exporting with the existing native TOML adapter."
            )),
        }
    }

    pub fn export(&self, document: &str, expose_secrets: bool) -> Result<StackV1, ExportError> {
        export(document, expose_secrets)
    }

    pub fn export_with_decisions(
        &self,
        document: &str,
        expose: impl FnMut(&str, usize, usize) -> Result<bool, ExportError>,
    ) -> Result<StackV1, ExportError> {
        export_with_decisions(document, expose)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn import_requires_checked_version() {
        let adapter = CodexImportAdapter::select(&CURRENT_STABLE).unwrap();
        let stack = StackV1::from_yaml("schema_version: 1\nservers: {}\n").unwrap();
        assert!(adapter.prepare(&stack, |_| None).unwrap().is_empty());
        for version in ["0.159.0", "0.160.1", "0.160.0-alpha.1", "1.0.0"] {
            assert!(matches!(
                CodexImportAdapter::select(&Version::parse(version).unwrap()),
                Err(AdapterError::UnsupportedImportVersion)
            ));
        }
    }

    #[test]
    fn resolves_windows_launchers_in_path_order() {
        let root = tempfile::tempdir().unwrap();
        let first = root.path().join("npm tools with spaces");
        let second = root.path().join("native tools");
        std::fs::create_dir_all(&first).unwrap();
        std::fs::create_dir_all(&second).unwrap();
        let path = std::env::join_paths([&first, &second]).unwrap();
        assert!(windows_launcher(&path).is_none());
        std::fs::write(first.join("codex.cmd"), "fixture").unwrap();
        std::fs::write(second.join("codex.exe"), "fixture").unwrap();
        assert_eq!(
            windows_launcher(&path).unwrap(),
            std::path::absolute(first.join("codex.cmd")).unwrap()
        );
        std::fs::write(first.join("codex.exe"), "fixture").unwrap();
        assert_eq!(
            windows_launcher(&path).unwrap(),
            std::path::absolute(first.join("codex.exe")).unwrap()
        );
    }

    #[test]
    fn parses_codex_version_without_exposing_bad_output() {
        assert_eq!(
            parse_version(b"codex-cli 0.149.0\r\n").unwrap(),
            Version::new(0, 149, 0)
        );
        for output in [
            b"".as_slice(),
            b"0.149.0",
            b"other 0.149.0",
            b"codex-cli fixture-secret",
            b"codex-cli 0.149.0 extra",
            b"\xff",
        ] {
            let error = parse_version(output).unwrap_err();
            assert_eq!(error, AdapterError::Detection);
            assert!(!error.to_string().contains("fixture-secret"));
        }
    }

    #[test]
    fn covers_every_pre1_version_through_current_stable() {
        for minor in 0..=160 {
            let version = Version::new(0, minor, 0);
            assert_eq!(CodexAdapter::select(&version).unwrap(), CodexAdapter::Pre1);
        }
        for version in [
            "0.0.0",
            "0.1.99",
            "0.149.1",
            "0.159.999",
            "0.160.0-alpha.1",
            "0.160.0+build.1",
        ] {
            assert!(
                CodexAdapter::select(&Version::parse(version).unwrap())
                    .unwrap()
                    .warning()
                    .is_none()
            );
        }
    }

    #[test]
    fn newer_versions_warn_but_can_export() {
        for version in ["0.160.1", "0.161.0-alpha.1", "0.161.0"] {
            let adapter = CodexAdapter::select(&Version::parse(version).unwrap()).unwrap();
            let warning = adapter.warning().unwrap();
            assert!(warning.contains(version));
            assert!(warning.contains("0.160.0"));
            assert!(
                adapter
                    .export("[mcp_servers.example]\ncommand='example'", false)
                    .is_ok()
            );
        }
    }
    #[test]
    fn rejects_major_releases_and_their_prereleases() {
        for version in [
            "1.0.0-alpha.1",
            "1.0.0",
            "1.0.0+build.1",
            "2.0.0-beta.1",
            "2.0.0",
        ] {
            assert_eq!(
                CodexAdapter::select(&Version::parse(version).unwrap()).unwrap_err(),
                AdapterError::UnsupportedMajor
            );
        }
    }
}
