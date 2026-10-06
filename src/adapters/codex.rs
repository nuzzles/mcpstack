use std::io;
#[cfg(any(target_os = "macos", test))]
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use semver::Version;
use thiserror::Error;

use crate::exporters::codex::{ExportError, export, export_with_decisions};
use crate::schema::StackV1;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum AdapterError {
    #[error(
        "Unable to detect Codex version. Ensure Codex is installed and codex --version succeeds."
    )]
    CodexDetection,
    #[error("Codex 1.0.0 and later, including prereleases, require an explicit supported adapter.")]
    UnsupportedCodexExportVersion,
    #[error("Import supports only the checked stable Codex version 0.160.0.")]
    UnsupportedCodexImportVersion,
}

/// Select an adapter without reading or modifying client configuration.
pub fn detect_codex() -> Result<CodexAdapter, AdapterError> {
    let version = detect_codex_version()?;
    CodexAdapter::select(&version)
}

fn detect_codex_version() -> Result<Version, AdapterError> {
    let output = match version_output(codex_command()?) {
        Ok(output) => output,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            version_output(bundled_codex_command().ok_or(AdapterError::CodexDetection)?)
                .map_err(|_| AdapterError::CodexDetection)?
        }
        Err(_) => return Err(AdapterError::CodexDetection),
    };
    if !output.status.success() {
        return Err(AdapterError::CodexDetection);
    }
    parse_version(&output.stdout)
}

fn version_output(mut command: Command) -> io::Result<Output> {
    command.arg("--version").stdin(Stdio::null()).output()
}

#[cfg(target_os = "macos")]
fn bundled_codex_command() -> Option<Command> {
    let home = directories::BaseDirs::new();
    let apps = codex_app_roots(
        std::env::var_os("MCPSTACK_CODEX_APP").map(PathBuf::from),
        Path::new("/Applications"),
        home.as_ref().map(|dirs| dirs.home_dir()),
    );
    find_bundled_codex(apps).map(Command::new)
}

// Keep candidate construction independent of the host filesystem so both
// standard application locations can be tested without touching installed apps.
#[cfg(any(target_os = "macos", test))]
fn codex_app_roots(
    explicit: Option<PathBuf>,
    applications: &Path,
    home: Option<&Path>,
) -> Vec<PathBuf> {
    if let Some(app) = explicit {
        return vec![app];
    }
    let mut apps = vec![applications.join("ChatGPT.app")];
    if let Some(home) = home {
        apps.push(home.join("Applications/ChatGPT.app"));
    }
    apps
}

#[cfg(any(target_os = "macos", test))]
fn find_bundled_codex(apps: Vec<PathBuf>) -> Option<PathBuf> {
    apps.into_iter()
        .map(|app| app.join("Contents/Resources/codex-cli/CodexCLI.app/Contents/MacOS/codex"))
        .find(|path| path.is_file())
}

#[cfg(not(target_os = "macos"))]
fn bundled_codex_command() -> Option<Command> {
    None
}

#[cfg(not(windows))]
fn codex_command() -> Result<Command, AdapterError> {
    Ok(Command::new("codex"))
}

#[cfg(windows)]
fn codex_command() -> Result<Command, AdapterError> {
    let path = std::env::var_os("PATH").ok_or(AdapterError::CodexDetection)?;
    let launcher = windows_launcher(&path).ok_or(AdapterError::CodexDetection)?;
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
    let output = std::str::from_utf8(output).map_err(|_| AdapterError::CodexDetection)?;
    let mut words = output.split_whitespace();
    if words.next() != Some("codex-cli") {
        return Err(AdapterError::CodexDetection);
    }
    let version = words.next().ok_or(AdapterError::CodexDetection)?;
    if words.next().is_some() {
        return Err(AdapterError::CodexDetection);
    }
    Version::parse(version).map_err(|_| AdapterError::CodexDetection)
}

/// Import adapter selected after checking the supported Codex version.
pub struct CodexImportAdapter;

#[allow(dead_code, reason = "foundation for the stacked import CLI PR")]
pub fn detect_import() -> Result<CodexImportAdapter, AdapterError> {
    CodexImportAdapter::select(&detect_codex_version()?)
}

impl CodexImportAdapter {
    fn select(version: &Version) -> Result<Self, AdapterError> {
        if version != &CURRENT_STABLE {
            return Err(AdapterError::UnsupportedCodexImportVersion);
        }
        Ok(Self)
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
            return Err(AdapterError::UnsupportedCodexExportVersion);
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
                Err(AdapterError::UnsupportedCodexImportVersion)
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
    fn finds_system_user_and_explicit_desktop_bundles() {
        let root = tempfile::tempdir().unwrap();
        let system = root.path().join("system Applications");
        let home = root.path().join("user home");
        let explicit = root.path().join("custom ChatGPT.app");
        let roots = codex_app_roots(None, &system, Some(&home));
        assert_eq!(
            roots,
            [
                system.join("ChatGPT.app"),
                home.join("Applications/ChatGPT.app")
            ]
        );
        assert!(find_bundled_codex(roots.clone()).is_none());
        let relative = "Contents/Resources/codex-cli/CodexCLI.app/Contents/MacOS/codex";
        let user_binary = roots[1].join(relative);
        std::fs::create_dir_all(user_binary.parent().unwrap()).unwrap();
        std::fs::write(&user_binary, "fixture").unwrap();
        assert_eq!(find_bundled_codex(roots.clone()), Some(user_binary));
        let system_binary = roots[0].join(relative);
        std::fs::create_dir_all(system_binary.parent().unwrap()).unwrap();
        std::fs::write(&system_binary, "fixture").unwrap();
        assert_eq!(find_bundled_codex(roots), Some(system_binary));
        assert_eq!(
            codex_app_roots(None, &system, None),
            [system.join("ChatGPT.app")]
        );
        let override_roots = codex_app_roots(Some(explicit.clone()), &system, Some(&home));
        assert_eq!(override_roots.as_slice(), std::slice::from_ref(&explicit));
        assert!(find_bundled_codex(override_roots.clone()).is_none());
        let binary = explicit.join(relative);
        std::fs::create_dir_all(binary.parent().unwrap()).unwrap();
        std::fs::write(&binary, "fixture").unwrap();
        assert_eq!(find_bundled_codex(override_roots), Some(binary));
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
            assert_eq!(error, AdapterError::CodexDetection);
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
                AdapterError::UnsupportedCodexExportVersion
            );
        }
    }
}
