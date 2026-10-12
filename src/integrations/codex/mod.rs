//! Codex configuration discovery, validation, conversion, and workflows.
mod config;
pub(super) mod diff;
pub(crate) mod export;
mod import;
mod schema;
pub(super) mod workflow;

use std::env;
use std::path::PathBuf;

use thiserror::Error;

use crate::error::{AppError, ErrorCode};

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
