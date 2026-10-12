//! Release discovery and best-effort update notices. Never handles client settings.

mod install;
#[cfg(test)]
mod tests;

pub use install::Installation;

use crate::error::ErrorCode;
use directories::BaseDirs;
use reqwest::Client;
use semver::Version;
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use thiserror::Error;

const RELEASES_URL: &str = "https://crates.io/api/v1/crates/mcpstack";
const CHECK_PERIOD: u64 = 24 * 60 * 60;
const NOTICE_TIMEOUT: Duration = Duration::from_millis(750);
const MAX_RESPONSE_BYTES: usize = 1024 * 1024;

#[derive(Debug, Error)]
pub enum Error {
    #[error("{code}", code = ErrorCode::UpdateCheckError)]
    Check,
    #[error(
        "Updating requires confirmation. Run interactively or use mcpstack update --non-interactive -y."
    )]
    ApprovalRequired,
    #[error(
        "Update confirmation was cancelled or could not be completed. mcpstack was not updated."
    )]
    ApprovalCancelled,
    #[error(
        "This executable is not a tracked Cargo installation. Install with cargo install mcpstack --locked, or update through your original package manager."
    )]
    Unmanaged,
    #[error(
        "Another update is running, or the installation changed. Wait for it to finish and run mcpstack update again."
    )]
    Busy,
    #[error("Unable to start Cargo. Install Rust and Cargo, then retry mcpstack update.")]
    CargoMissing,
    #[error(
        "Cargo could not install the update. Review its diagnostics and retry mcpstack update."
    )]
    Install,
    #[error(
        "Unable to prepare the executable for replacement. Check installation permissions and retry."
    )]
    Prepare,
    #[cfg(windows)]
    #[error(
        "Could not restore the executable after a failed replacement. Reinstall with cargo install mcpstack --locked --force using the reported install root."
    )]
    Restore,
    #[error(
        "Cargo finished, but the installed version could not be verified. Run mcpstack --version and check the reported install root."
    )]
    Verify,
}

impl Error {
    pub fn code(&self) -> ErrorCode {
        match self {
            Self::Check => ErrorCode::UpdateCheckError,
            Self::ApprovalRequired => ErrorCode::InvalidArgument,
            _ => ErrorCode::UpdateError,
        }
    }
}

pub fn current_version() -> Version {
    Version::parse(env!("CARGO_PKG_VERSION")).expect("Cargo supplies a semantic package version")
}

pub fn is_newer(latest: &Version, current: &Version) -> bool {
    latest.cmp_precedence(current).is_gt()
}

#[derive(Deserialize)]
struct Releases {
    versions: Vec<Release>,
}

#[derive(Deserialize)]
struct Release {
    num: String,
    yanked: bool,
}

fn latest_stable(releases: Releases) -> Option<Version> {
    releases
        .versions
        .into_iter()
        .filter(|version| !version.yanked)
        .filter_map(|version| Version::parse(&version.num).ok())
        .filter(|version| version.pre.is_empty())
        .max_by(Version::cmp_precedence)
}

pub async fn latest() -> Result<Option<Version>, Error> {
    fetch_latest(RELEASES_URL, Duration::from_secs(10)).await
}

async fn fetch_latest(url: &str, timeout: Duration) -> Result<Option<Version>, Error> {
    // Bound the entire operation, including the body, not just connection setup.
    tokio::time::timeout(timeout, async {
        let client = Client::builder()
            .user_agent(concat!(
                env!("CARGO_PKG_NAME"),
                "/",
                env!("CARGO_PKG_VERSION"),
                " (",
                env!("CARGO_PKG_REPOSITORY"),
                ")"
            ))
            .timeout(timeout)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| Error::Check)?;
        let mut response = client.get(url).send().await.map_err(|_| Error::Check)?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !response.status().is_success() {
            return Err(Error::Check);
        }
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| Error::Check)? {
            if body.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
                return Err(Error::Check);
            }
            body.extend_from_slice(&chunk);
        }
        let releases = serde_json::from_slice(&body).map_err(|_| Error::Check)?;
        Ok(latest_stable(releases))
    })
    .await
    .map_err(|_| Error::Check)?
}

#[derive(Deserialize, Serialize)]
struct Cache {
    checked_at: u64,
    latest: Option<Version>,
}

impl Cache {
    fn fresh(&self, now: u64) -> bool {
        // A clock rollback must not suppress checks indefinitely.
        now.checked_sub(self.checked_at)
            .is_some_and(|elapsed| elapsed < CHECK_PERIOD)
    }
}

fn read_cache(path: &Path) -> Option<Cache> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .ok()?
        .take(4096)
        .read_to_end(&mut bytes)
        .ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn write_cache(path: &Path, cache: &Cache) -> std::io::Result<()> {
    let parent = path.parent().ok_or(std::io::ErrorKind::InvalidInput)?;
    std::fs::create_dir_all(parent)?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    serde_json::to_writer(file.as_file_mut(), cache)?;
    file.persist(path)?;
    Ok(())
}

async fn cached_latest(path: &Path, now: u64, url: &str) -> Option<Version> {
    if let Some(cache) = read_cache(path)
        && cache.fresh(now)
    {
        return cache.latest;
    }
    // Record the attempt first: the process can exit while an advisory request
    // is still waiting on DNS, certificates, or an unavailable registry.
    let _ = write_cache(
        path,
        &Cache {
            checked_at: now,
            latest: None,
        },
    );
    let latest = fetch_latest(url, NOTICE_TIMEOUT).await.ok().flatten();
    // Also back off after network failures or before the first release exists.
    // Unwritable caches never turn a successful client operation into a failure.
    let _ = write_cache(
        path,
        &Cache {
            checked_at: now,
            latest: latest.clone(),
        },
    );
    latest
}

pub async fn notify() {
    let Some(dirs) = BaseDirs::new() else { return };
    let Ok(now) = SystemTime::now().duration_since(UNIX_EPOCH) else {
        return;
    };
    let path = dirs.cache_dir().join("mcpstack").join("update.json");
    let (sender, receiver) = tokio::sync::oneshot::channel();
    // Like Trunk, isolate the update runtime. DNS uses blocking system calls;
    // aborting a task on the main runtime could still delay process shutdown.
    let worker = std::thread::Builder::new()
        .name("mcpstack-update".into())
        .spawn(move || {
            if let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                let latest = runtime.block_on(cached_latest(&path, now.as_secs(), RELEASES_URL));
                let _ = sender.send(latest);
            }
        });
    if worker.is_err() {
        return;
    }
    if let Ok(Ok(Some(latest))) = tokio::time::timeout(NOTICE_TIMEOUT, receiver).await
        && is_newer(&latest, &current_version())
    {
        // Advisory output is best effort and always separate from command stdout.
        let _ = writeln!(
            std::io::stderr().lock(),
            "Update available: mcpstack {} -> {latest}. Run mcpstack update to install it.",
            current_version()
        );
    }
}
