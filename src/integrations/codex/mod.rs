//! Codex configuration discovery, validation, conversion, and workflows.
mod config;
mod diff;
mod export;
mod import;
mod schema;
mod workflow;

use std::env;
use std::io::Write;
use std::path::PathBuf;

use clap::Args;
use thiserror::Error;

use crate::error::{AppError, ErrorCode};

pub(super) const EXAMPLES: &[&str] = &[
    "mcpstack export codex --help",
    "mcpstack import codex --help",
    "mcpstack diff codex --help",
];

#[derive(Args)]
pub struct ExportArgs {
    /// Read this Codex TOML file instead of the default configuration.
    #[arg(long, value_name = "PATH")]
    config: Option<PathBuf>,
}

impl ExportArgs {
    pub(super) async fn run(
        self,
        output: &mut impl Write,
        non_interactive: bool,
        expose_secrets: bool,
    ) -> Result<(), AppError> {
        workflow::run_export(self.config, output, non_interactive, expose_secrets).await
    }
}

#[derive(Args)]
pub struct StackArgs {
    /// Stack file to compare or import.
    #[arg(value_name = "FILE")]
    file: PathBuf,
    /// Use this Codex TOML file instead of the default configuration.
    #[arg(long, value_name = "PATH")]
    config: Option<PathBuf>,
}

impl StackArgs {
    pub(super) async fn import(
        self,
        output: &mut impl Write,
        non_interactive: bool,
        colored: bool,
        dry_run: bool,
        auto_approve: bool,
    ) -> Result<(), AppError> {
        workflow::run_import(
            self.file,
            self.config,
            output,
            non_interactive,
            colored,
            dry_run,
            auto_approve,
        )
        .await
    }

    pub(super) async fn diff(
        self,
        output: &mut impl Write,
        non_interactive: bool,
        colored: bool,
    ) -> Result<(), AppError> {
        diff::run(self.file, self.config, output, non_interactive, colored).await
    }
}

fn default_config() -> Option<PathBuf> {
    if let Some(home) = env::var_os("CODEX_HOME").filter(|value| !value.is_empty()) {
        Some(PathBuf::from(home).join("config.toml"))
    } else {
        directories::BaseDirs::new().map(|dirs| dirs.home_dir().join(".codex/config.toml"))
    }
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("Cannot determine the Codex config path. Set CODEX_HOME.")]
    ConfigPath,
    #[error(transparent)]
    Export(#[from] export::ExportError),
    #[error(transparent)]
    Import(#[from] import::ImportError),
    #[error(transparent)]
    File(#[from] config::FileError),
}

impl Error {
    pub(super) fn code(&self) -> ErrorCode {
        match self {
            Self::ConfigPath => ErrorCode::ConfigReadError,
            Self::Export(_) => ErrorCode::ExportError,
            Self::Import(_) => ErrorCode::ImportError,
            Self::File(config::FileError::Backup) => ErrorCode::BackupError,
            Self::File(_) => ErrorCode::ConfigWriteError,
        }
    }
}

impl From<Error> for AppError {
    fn from(error: Error) -> Self {
        Self::Integration(error.into())
    }
}

impl From<export::ExportError> for AppError {
    fn from(error: export::ExportError) -> Self {
        Error::Export(error).into()
    }
}

impl From<import::ImportError> for AppError {
    fn from(error: import::ImportError) -> Self {
        Error::Import(error).into()
    }
}

impl From<config::FileError> for AppError {
    fn from(error: config::FileError) -> Self {
        Error::File(error).into()
    }
}
