//! Integration registration and client-independent dispatch.
mod codex;

use std::io::Write;

use clap::Subcommand;
use thiserror::Error;

use crate::error::{AppError, ErrorCode};

pub const EXAMPLES: &[&str] = &[
    "mcpstack --schema",
    "mcpstack --help",
    "mcpstack validate --help",
    codex::EXAMPLES[0],
    codex::EXAMPLES[1],
    codex::EXAMPLES[2],
];

#[derive(Subcommand)]
pub enum ExportClient {
    Codex(codex::ExportArgs),
}

impl ExportClient {
    pub async fn run(
        self,
        output: &mut impl Write,
        non_interactive: bool,
        expose_secrets: bool,
    ) -> Result<(), AppError> {
        match self {
            Self::Codex(args) => args.run(output, non_interactive, expose_secrets).await,
        }
    }
}

#[derive(Subcommand)]
pub enum StackClient {
    Codex(codex::StackArgs),
}

impl StackClient {
    pub async fn import(
        self,
        output: &mut impl Write,
        non_interactive: bool,
        colored: bool,
        dry_run: bool,
        auto_approve: bool,
    ) -> Result<(), AppError> {
        match self {
            Self::Codex(args) => {
                args.import(output, non_interactive, colored, dry_run, auto_approve)
                    .await
            }
        }
    }

    pub async fn diff(
        self,
        output: &mut impl Write,
        non_interactive: bool,
        colored: bool,
    ) -> Result<(), AppError> {
        match self {
            Self::Codex(args) => args.diff(output, non_interactive, colored).await,
        }
    }
}

#[derive(Debug, Error)]
pub enum Error {
    #[error(transparent)]
    Codex(#[from] codex::Error),
}

impl Error {
    pub fn code(&self) -> ErrorCode {
        match self {
            Self::Codex(error) => error.code(),
        }
    }
}
