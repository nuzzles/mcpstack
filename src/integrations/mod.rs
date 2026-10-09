//! Client selection and adapter dispatch.
mod claude;
mod codex;
mod reserved;
mod secrets;

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
    "mcpstack claude --help",
    "mcpstack claude export --help",
];

#[derive(Args)]
pub struct ExportArgs {
    /// Read this client config file instead of the default configuration.
    #[arg(long, value_name = "PATH")]
    config: Option<PathBuf>,
    /// Include the otherwise skipped workspace server.
    #[arg(long)]
    allow_workspace: bool,
    /// Include the otherwise skipped claude-in-chrome server.
    #[arg(long)]
    allow_claude_in_chrome: bool,
    /// Include the otherwise skipped computer-use server.
    #[arg(long)]
    allow_computer_use: bool,
    /// Include the otherwise skipped Claude Preview server.
    #[arg(long)]
    allow_claude_preview: bool,
    /// Include the otherwise skipped Claude Browser server.
    #[arg(long)]
    allow_claude_browser: bool,
    /// Include the otherwise skipped node_repl server.
    #[arg(long)]
    allow_node_repl: bool,
}

impl ExportArgs {
    fn filter(&self) -> reserved::ExportFilter {
        reserved::ExportFilter {
            workspace: self.allow_workspace,
            claude_in_chrome: self.allow_claude_in_chrome,
            computer_use: self.allow_computer_use,
            claude_preview: self.allow_claude_preview,
            claude_browser: self.allow_claude_browser,
            node_repl: self.allow_node_repl,
        }
    }
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
    /// Manage MCP servers in Claude Code user configuration.
    Claude(ClientCommands),
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
        match self {
            Self::Codex(commands) => {
                run_commands(&Codex, commands, output, non_interactive, colored).await
            }
            Self::Claude(commands) => {
                run_commands(&Claude, commands, output, non_interactive, colored).await
            }
        }
    }
}

async fn run_commands(
    adapter: &impl ClientAdapter,
    commands: ClientCommands,
    output: &mut impl Write,
    non_interactive: bool,
    colored: bool,
) -> Result<(), AppError> {
    match commands.operation {
        Operation::Use(args) => args.run(adapter, output, non_interactive, colored).await,
        Operation::Import(args) => args.run(adapter, output, non_interactive, colored).await,
        Operation::Export(args) => args.run(adapter, output, non_interactive).await,
        Operation::Diff(args) => args.run(adapter, output, non_interactive, colored).await,
    }
}

pub trait ClientAdapter {
    async fn export(
        &self,
        args: ExportArgs,
        output: &mut impl Write,
        non_interactive: bool,
        expose_secrets: bool,
    ) -> Result<(), AppError>;
    async fn import(
        &self,
        args: StackArgs,
        output: &mut impl Write,
        non_interactive: bool,
        colored: bool,
        dry_run: bool,
        auto_approve: bool,
    ) -> Result<(), AppError>;
    async fn use_stack(
        &self,
        args: StackArgs,
        output: &mut impl Write,
        non_interactive: bool,
        colored: bool,
        dry_run: bool,
        auto_approve: bool,
    ) -> Result<(), AppError>;
    async fn diff(
        &self,
        args: StackArgs,
        output: &mut impl Write,
        non_interactive: bool,
        colored: bool,
    ) -> Result<(), AppError>;
}

pub struct Codex;
pub struct Claude;

impl ClientAdapter for Codex {
    async fn export(
        &self,
        args: ExportArgs,
        output: &mut impl Write,
        non_interactive: bool,
        expose_secrets: bool,
    ) -> Result<(), AppError> {
        let filter = args.filter();
        codex::workflow::run_export(args.config, filter, output, non_interactive, expose_secrets)
            .await
    }

    async fn import(
        &self,
        args: StackArgs,
        output: &mut impl Write,
        non_interactive: bool,
        colored: bool,
        dry_run: bool,
        auto_approve: bool,
    ) -> Result<(), AppError> {
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

    async fn use_stack(
        &self,
        args: StackArgs,
        output: &mut impl Write,
        non_interactive: bool,
        colored: bool,
        dry_run: bool,
        auto_approve: bool,
    ) -> Result<(), AppError> {
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

    async fn diff(
        &self,
        args: StackArgs,
        output: &mut impl Write,
        non_interactive: bool,
        colored: bool,
    ) -> Result<(), AppError> {
        codex::diff::run(args.file, args.config, output, non_interactive, colored).await
    }
}

#[derive(Debug, Error)]
pub enum Error {
    #[error(transparent)]
    Codex(#[from] codex::Error),
    #[error(transparent)]
    Claude(#[from] claude::Error),
}

impl Error {
    pub fn code(&self) -> ErrorCode {
        match self {
            Self::Codex(error) => error.code(),
            Self::Claude(error) => error.code(),
        }
    }
}
