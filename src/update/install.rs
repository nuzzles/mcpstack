//! Delegate building, replacement, and Cargo bookkeeping to `cargo install`.

use super::{Error, current_version};
use semver::Version;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs::{self, OpenOptions};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub struct Installation {
    executable: PathBuf,
    root: PathBuf,
}

#[derive(Deserialize)]
struct CargoInstalls {
    v1: BTreeMap<String, Vec<String>>,
}

impl Installation {
    pub fn current() -> Result<Self, Error> {
        let executable = std::env::current_exe().map_err(|_| Error::Unmanaged)?;
        Self::from_executable(executable)
    }

    fn from_executable(executable: PathBuf) -> Result<Self, Error> {
        let bin = executable.parent().ok_or(Error::Unmanaged)?;
        let root = bin.parent().ok_or(Error::Unmanaged)?.to_owned();
        if bin.file_name() != Some(OsStr::new("bin"))
            || executable.file_name() != Some(OsStr::new(binary_name()))
        {
            return Err(Error::Unmanaged);
        }
        let mut metadata = String::new();
        fs::File::open(root.join(".crates.toml"))
            .map_err(|_| Error::Unmanaged)?
            .take(1024 * 1024)
            .read_to_string(&mut metadata)
            .map_err(|_| Error::Unmanaged)?;
        let installs: CargoInstalls = toml::from_str(&metadata).map_err(|_| Error::Unmanaged)?;
        let current = current_version().to_string();
        let tracked = installs.v1.iter().any(|(package, binaries)| {
            let mut parts = package.splitn(3, ' ');
            parts.next() == Some("mcpstack")
                && parts.next() == Some(current.as_str())
                && parts.next().is_some()
                && binaries.iter().any(|binary| binary == binary_name())
        });
        if !tracked {
            return Err(Error::Unmanaged);
        }
        Ok(Self { executable, root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn install(&self, version: &Version) -> Result<(), Error> {
        self.install_with(OsStr::new("cargo"), version)
    }

    fn install_with(&self, cargo: &OsStr, version: &Version) -> Result<(), Error> {
        // Lock only our updater: Cargo owns the locks on its installation metadata.
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(self.root.join(".mcpstack-update.lock"))
            .map_err(|_| Error::Prepare)?;
        lock.try_lock().map_err(|_| Error::Busy)?;
        Self::from_executable(self.executable.clone()).map_err(|_| Error::Busy)?;
        // Avoid repository-local Cargo config and target directories. Cargo retains
        // its normal user configuration and manages .crates.toml/.crates2.json.
        let working = tempfile::tempdir().map_err(|_| Error::Prepare)?;
        let mut cargo = Command::new(cargo);
        if let Some(home) = std::env::var_os("CARGO_HOME").filter(|home| !home.is_empty()) {
            // Relative Cargo homes refer to the caller's directory, not our
            // temporary working directory.
            cargo.env(
                "CARGO_HOME",
                std::path::absolute(home).map_err(|_| Error::Prepare)?,
            );
        }
        cargo
            .current_dir(working.path())
            .args([
                "install",
                "mcpstack",
                "--locked",
                "--registry",
                "crates-io",
                "--version",
            ])
            .arg(format!("={version}"))
            .arg("--root")
            .arg(&self.root)
            .stdin(Stdio::null())
            .stdout(Stdio::from(std::io::stderr()))
            .stderr(Stdio::inherit());
        // On Windows an executing image cannot be replaced by Cargo. Relocate
        // that image first while leaving an identical, runnable copy in place.
        // self-replace cleans the running copy up after this process exits.
        #[cfg(windows)]
        prepare_windows(&self.executable)?;
        let status = cargo.status().map_err(|_| Error::CargoMissing)?;
        if !status.success() {
            return Err(Error::Install);
        }
        let verified = Command::new(&self.executable)
            .arg("--version")
            .stdin(Stdio::null())
            .output()
            .map_err(|_| Error::Verify)?;
        let expected = format!("mcpstack {version}");
        if !verified.status.success()
            || String::from_utf8_lossy(&verified.stdout).trim() != expected
        {
            return Err(Error::Verify);
        }
        Ok(())
    }
}

fn binary_name() -> &'static str {
    if cfg!(windows) {
        "mcpstack.exe"
    } else {
        "mcpstack"
    }
}

#[cfg(windows)]
fn prepare_windows(executable: &Path) -> Result<(), Error> {
    let snapshot = tempfile::tempdir().map_err(|_| Error::Prepare)?;
    let copy = snapshot.path().join("mcpstack.exe");
    fs::copy(executable, &copy).map_err(|_| Error::Prepare)?;
    if self_replace::self_replace(&copy).is_err() {
        // self-replace can fail after relocating the running image. Keep the
        // original installation usable even when its cleanup helper cannot start.
        if !executable.exists() {
            fs::copy(&copy, executable).map_err(|_| Error::Restore)?;
        }
        return Err(Error::Prepare);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::OnceLock;

    fn metadata(root: &Path) {
        fs::create_dir_all(root.join("bin")).unwrap();
        fs::write(
            root.join(".crates.toml"),
            format!(
                "[v1]\n\"mcpstack {} (path+file:///source)\" = [\"{}\"]\n",
                current_version(),
                binary_name()
            ),
        )
        .unwrap();
    }

    #[test]
    fn detects_custom_cargo_roots_and_rejects_untracked_or_mismatched_binaries() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("custom root");
        let exe = root.join("bin").join(binary_name());
        assert!(Installation::from_executable(exe.clone()).is_err());
        metadata(&root);
        assert_eq!(
            Installation::from_executable(exe.clone()).unwrap().root(),
            root
        );
        assert!(Installation::from_executable(root.join(binary_name())).is_err());
        assert!(Installation::from_executable(root.join("bin/another-tool")).is_err());
        fs::write(
            root.join(".crates.toml"),
            format!(
                "[v1]\n\"other 0.1.0 (registry+index)\" = [\"{}\"]\n",
                binary_name()
            ),
        )
        .unwrap();
        assert!(Installation::from_executable(exe.clone()).is_err());
        fs::write(
            root.join(".crates.toml"),
            format!(
                "[v1]\n\"mcpstack 999.0.0 (registry+index)\" = [\"{}\"]\n",
                binary_name()
            ),
        )
        .unwrap();
        assert!(Installation::from_executable(exe).is_err());
    }

    // Compile a tiny stand-in for Cargo once. Unlike shell scripts this also
    // exercises executable replacement on Windows and paths containing spaces.
    fn cargo_fixture() -> &'static Path {
        static HELPER: OnceLock<tempfile::TempDir> = OnceLock::new();
        HELPER.get_or_init(|| {
            let dir = tempfile::tempdir().unwrap();
            let source = dir.path().join("cargo_fixture.rs");
            fs::write(&source, r#"
                use std::{env, fs, path::PathBuf, process};
                fn main() {
                    let args: Vec<_> = env::args().collect();
                    if args.get(1).map(String::as_str) == Some("--version") {
                        println!("mcpstack 0.10.0");
                        return;
                    }
                    assert_eq!(&args[1..8], &["install", "mcpstack", "--locked", "--registry", "crates-io", "--version", "=0.10.0"]);
                    assert_eq!(args[8], "--root");
                    assert_eq!(args.len(), 10);
                    println!("cargo progress on stdout");
                    eprintln!("cargo progress on stderr");
                    let root = PathBuf::from(&args[9]);
                    assert_ne!(env::current_dir().unwrap(), root);
                    assert!(PathBuf::from(env::var_os("CARGO_HOME").unwrap()).is_absolute());
                    if args[0].contains("fail") { process::exit(1); }
                    if args[0].contains("wrong") { return; }
                    let binary = if cfg!(windows) { "mcpstack.exe" } else { "mcpstack" };
                    let staged = root.join("bin/staged");
                    fs::copy(env::current_exe().unwrap(), &staged).unwrap();
                    fs::rename(staged, root.join("bin").join(binary)).unwrap();
                }
            "#).unwrap();
            let helper = dir.path().join(format!("cargo{}", std::env::consts::EXE_SUFFIX));
            let built = Command::new("rustc").arg(&source).arg("-o").arg(helper).output().unwrap();
            assert!(built.status.success(), "{built:?}");
            dir
        }).path()
    }

    // A copied test executable runs this case as the installed application. This
    // validates replacing the running image, rather than an unlocked fixture file.
    #[test]
    fn update_child() {
        let Some(cargo) = std::env::var_os("MCPSTACK_TEST_CARGO") else {
            return;
        };
        let result = Installation::current()
            .unwrap()
            .install_with(&cargo, &Version::new(0, 10, 0));
        match std::env::var("MCPSTACK_TEST_RESULT").unwrap().as_str() {
            "success" => result.unwrap(),
            "install" => assert!(matches!(result, Err(Error::Install))),
            "missing" => assert!(matches!(result, Err(Error::CargoMissing))),
            "verify" => assert!(matches!(result, Err(Error::Verify))),
            "busy" => assert!(matches!(result, Err(Error::Busy))),
            value => panic!("Unexpected scenario: {value}"),
        }
    }

    #[test]
    fn installer_updates_running_binary_preserves_failed_installs_and_keeps_logs_off_stdout() {
        for scenario in ["success", "install", "missing", "verify", "busy"] {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().join("cargo root with spaces");
            metadata(&root);
            let executable = root.join("bin").join(binary_name());
            fs::copy(std::env::current_exe().unwrap(), &executable).unwrap();
            let before = fs::read(&executable).unwrap();
            let cargo_name = match scenario {
                "install" => "cargo-fail",
                "verify" => "cargo-wrong",
                _ => "cargo",
            };
            let cargo = temp
                .path()
                .join(format!("{cargo_name}{}", std::env::consts::EXE_SUFFIX));
            if scenario != "missing" {
                fs::copy(
                    cargo_fixture().join(format!("cargo{}", std::env::consts::EXE_SUFFIX)),
                    &cargo,
                )
                .unwrap();
            }
            let lock = OpenOptions::new()
                .create(true)
                .truncate(false)
                .read(true)
                .write(true)
                .open(root.join(".mcpstack-update.lock"))
                .unwrap();
            if scenario == "busy" {
                lock.try_lock().unwrap();
            }
            let child = Command::new(&executable)
                .args([
                    "--exact",
                    "update::install::tests::update_child",
                    "--nocapture",
                ])
                .current_dir(temp.path())
                .env("CARGO_HOME", "cargo home")
                .env("MCPSTACK_TEST_CARGO", &cargo)
                .env("MCPSTACK_TEST_RESULT", scenario)
                .output()
                .unwrap();
            assert!(child.status.success(), "{scenario}: {child:?}");
            assert!(!String::from_utf8_lossy(&child.stdout).contains("cargo progress"));
            if matches!(scenario, "success" | "install" | "verify") {
                let stderr = String::from_utf8_lossy(&child.stderr);
                assert!(
                    stderr.contains("cargo progress on stdout"),
                    "{scenario}: {child:?}"
                );
                assert!(
                    stderr.contains("cargo progress on stderr"),
                    "{scenario}: {child:?}"
                );
            }
            if scenario == "success" {
                let version = Command::new(&executable).arg("--version").output().unwrap();
                assert!(version.status.success());
                assert_eq!(
                    String::from_utf8_lossy(&version.stdout).trim(),
                    "mcpstack 0.10.0"
                );
            } else {
                assert_eq!(fs::read(&executable).unwrap(), before, "{scenario}");
            }
        }
    }
}
