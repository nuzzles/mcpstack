//! Client selection and adapter dispatch.
mod codex;

use std::io::Write;
use std::path::PathBuf;

use clap::{Args, Subcommand};
use thiserror::Error;

use crate::cmd::{diff::Diff, export::Export, import::Import, r#use::Use};
use crate::error::{AppError, ErrorCode};

pub const EXAMPLES: &[&str] = &[
    "mcpstack --schema",
    "mcpstack --help",
    "mcpstack validate --help",
    "mcpstack codex --help",
    "mcpstack codex use --help",
    "mcpstack codex use work.yml --dry-run",
    "mcpstack codex export --help",
    "mcpstack codex import --help",
    "mcpstack codex diff --help",
];

#[derive(Args)]
pub struct ExportArgs {
    /// Read this client config file instead of the default configuration.
    #[arg(long, value_name = "PATH")]
    config: Option<PathBuf>,
}

#[derive(Args)]
pub struct StackArgs {
    /// Stack file to compare, import, or use.
    #[arg(value_name = "FILE")]
    file: PathBuf,
    /// Use this client config file instead of the default configuration.
    #[arg(long, value_name = "PATH")]
    config: Option<PathBuf>,
}

#[derive(Subcommand)]
pub enum Client {
    /// Manage MCP servers in Codex configuration.
    Codex(ClientCommands),
}

#[derive(Args)]
pub struct ClientCommands {
    #[command(subcommand)]
    operation: Operation,
}

#[derive(Subcommand)]
enum Operation {
    Use(Use),
    Import(Import),
    Export(Export),
    Diff(Diff),
}

impl Client {
    pub async fn run(
        self,
        output: &mut impl Write,
        non_interactive: bool,
        colored: bool,
    ) -> Result<(), AppError> {
        let (target, commands) = match self {
            Self::Codex(commands) => (Target::Codex, commands),
        };
        match commands.operation {
            Operation::Use(args) => args.run(target, output, non_interactive, colored).await,
            Operation::Import(args) => args.run(target, output, non_interactive, colored).await,
            Operation::Export(args) => args.run(target, output, non_interactive).await,
            Operation::Diff(args) => args.run(target, output, non_interactive, colored).await,
        }
    }
}

/// Selected by the parent command, never stored in a stack file.
pub enum Target {
    Codex,
}

impl Target {
    pub async fn export(
        self,
        args: ExportArgs,
        output: &mut impl Write,
        non_interactive: bool,
        expose_secrets: bool,
    ) -> Result<(), AppError> {
        match self {
            Self::Codex => {
                codex::workflow::run_export(args.config, output, non_interactive, expose_secrets)
                    .await
            }
        }
    }

    pub async fn import(
        self,
        args: StackArgs,
        output: &mut impl Write,
        non_interactive: bool,
        colored: bool,
        dry_run: bool,
        auto_approve: bool,
    ) -> Result<(), AppError> {
        match self {
            Self::Codex => {
                codex::workflow::run_import(
                    args.file,
                    args.config,
                    output,
                    non_interactive,
                    colored,
                    dry_run,
                    auto_approve,
                )
                .await
            }
        }
    }

    pub async fn use_stack(
        self,
        args: StackArgs,
        output: &mut impl Write,
        non_interactive: bool,
        colored: bool,
        dry_run: bool,
        auto_approve: bool,
    ) -> Result<(), AppError> {
        match self {
            Self::Codex => {
                codex::workflow::run_use(
                    args.file,
                    args.config,
                    output,
                    non_interactive,
                    colored,
                    dry_run,
                    auto_approve,
                )
                .await
            }
        }
    }

    pub async fn diff(
        self,
        args: StackArgs,
        output: &mut impl Write,
        non_interactive: bool,
        colored: bool,
    ) -> Result<(), AppError> {
        match self {
            Self::Codex => {
                codex::diff::run(args.file, args.config, output, non_interactive, colored).await
            }
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
