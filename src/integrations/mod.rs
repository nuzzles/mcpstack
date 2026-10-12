//! Client selection and adapter dispatch.
mod claude;
mod codex;
mod diff_output;
mod listing;
mod paths;
mod portable;
pub(crate) mod reserved;
mod secrets;
pub(crate) mod selection;

use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Path, PathBuf};

use clap::{Args, Subcommand};
use thiserror::Error;

use crate::cmd::{
    diff::Diff,
    export::{ClaudeExport, CodexExport},
    import::Import,
    list::List,
    show::{ClaudeShow, CodexShow},
    r#use::Use,
};
use crate::error::{AppError, ErrorCode};

fn write_change_summary(
    output: &mut impl Write,
    target: &Path,
    added: &BTreeSet<String>,
    modified: &BTreeSet<String>,
    deleted: &BTreeSet<String>,
    colored: bool,
) -> std::io::Result<()> {
    writeln!(
        output,
        "Target config: {}",
        std::path::absolute(target)?.display()
    )?;
    writeln!(output, "MCP server changes:")?;
    for (marker, label, color, names) in [
        ("+", "Added", 32, added),
        ("~", "Modified", 33, modified),
        ("-", "Deleted", 31, deleted),
    ] {
        let entries = if names.is_empty() {
            "none".to_owned()
        } else {
            names
                .iter()
                .map(|name| format!("{name:?}"))
                .collect::<Vec<_>>()
                .join(", ")
        };
        if colored && !names.is_empty() {
            writeln!(output, "\x1b[{color}m  {marker} {label}: {entries}\x1b[0m")?;
        } else {
            writeln!(output, "  {marker} {label}: {entries}")?;
        }
    }
    Ok(())
}

pub const EXAMPLES: &[&str] = &[
    "mcpstack --schema",
    "mcpstack --help",
    "mcpstack validate --help",
    "mcpstack codex --help",
    "mcpstack codex list --help",
    "mcpstack codex use --help",
    "mcpstack codex use work.yml --dry-run",
    "mcpstack codex show --help",
    "mcpstack codex export --help",
    "mcpstack codex import --help",
    "mcpstack codex diff --help",
    "mcpstack claude --help",
    "mcpstack claude list --help",
    "mcpstack claude show --help",
    "mcpstack claude export --help",
];

#[derive(Args)]
pub struct ExportArgs {
    /// Read this client config file instead of the default configuration.
    #[arg(long, value_name = "PATH")]
    config: Option<PathBuf>,
    /// Include per-server MCP tool permissions when available.
    #[arg(long)]
    include_permissions: bool,
    #[arg(skip)]
    pub(crate) permissions_config: Option<PathBuf>,
    #[arg(skip)]
    pub(crate) server_filter: reserved::ServerFilter,
}

impl ExportArgs {
    pub(crate) fn config_path(&self) -> Option<&Path> {
        self.config.as_deref()
    }

    fn includes_permissions(&self) -> bool {
        self.server_filter.all || self.include_permissions
    }
    fn filter(&self) -> reserved::ServerFilter {
        self.server_filter.clone()
    }
}

#[derive(Args)]
pub struct StackArgs {
    #[arg(skip)]
    pub(crate) selection: selection::Selection,
    /// Stack file to compare, import, or use.
    #[arg(value_name = "FILE")]
    file: PathBuf,
    /// Use this client config file instead of the default configuration.
    #[arg(long, value_name = "PATH")]
    config: Option<PathBuf>,
    /// Apply or compare permission rules stored in the stack.
    #[arg(long)]
    include_permissions: bool,
    /// Use this Claude Code settings file for permission rules.
    #[arg(long, value_name = "PATH", requires = "include_permissions")]
    permissions_config: Option<PathBuf>,
}

#[derive(Subcommand)]
pub enum Client {
    /// Manage MCP servers in Codex configuration.
    Codex(CodexCommands),
    /// Manage MCP servers in Claude Code user configuration.
    Claude(ClaudeCommands),
}

#[derive(Args)]
#[command(arg_required_else_help = false)]
pub struct CodexCommands {
    #[command(subcommand)]
    operation: CodexOperation,
}

#[derive(Subcommand)]
enum CodexOperation {
    List(List<reserved::CodexFilterArgs>),
    Use(Use),
    Import(Import),
    Show(CodexShow),
    Export(CodexExport),
    Diff(Diff),
}

#[derive(Args)]
#[command(arg_required_else_help = false)]
pub struct ClaudeCommands {
    #[command(subcommand)]
    operation: ClaudeOperation,
}

#[derive(Subcommand)]
enum ClaudeOperation {
    List(List<reserved::ClaudeFilterArgs>),
    Use(Use),
    Import(Import),
    Show(ClaudeShow),
    Export(ClaudeExport),
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
            Self::Codex(commands) => match commands.operation {
                CodexOperation::List(args) => args.run(&Codex, output, colored).await,
                CodexOperation::Use(args) => {
                    args.run(&Codex, output, non_interactive, colored).await
                }
                CodexOperation::Import(args) => {
                    args.run(&Codex, output, non_interactive, colored).await
                }
                CodexOperation::Show(args) => args.run(&Codex, output, non_interactive).await,
                CodexOperation::Export(args) => args.run(&Codex, output, non_interactive).await,
                CodexOperation::Diff(args) => {
                    args.run(&Codex, output, non_interactive, colored).await
                }
            },
            Self::Claude(commands) => match commands.operation {
                ClaudeOperation::List(args) => args.run(&Claude, output, colored).await,
                ClaudeOperation::Use(args) => {
                    args.run(&Claude, output, non_interactive, colored).await
                }
                ClaudeOperation::Import(args) => {
                    args.run(&Claude, output, non_interactive, colored).await
                }
                ClaudeOperation::Show(args) => args.run(&Claude, output, non_interactive).await,
                ClaudeOperation::Export(args) => args.run(&Claude, output, non_interactive).await,
                ClaudeOperation::Diff(args) => {
                    args.run(&Claude, output, non_interactive, colored).await
                }
            },
        }
    }
}

pub trait ClientAdapter {
    async fn list(
        &self,
        config: Option<PathBuf>,
        filter: reserved::ServerFilter,
        output: &mut impl Write,
        colored: bool,
    ) -> Result<(), AppError>;

    async fn show(
        &self,
        args: ExportArgs,
        output: &mut impl Write,
        non_interactive: bool,
        expose_secrets: bool,
    ) -> Result<(), AppError> {
        self.export(args, output, non_interactive, expose_secrets)
            .await
    }

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
    async fn list(
        &self,
        config: Option<PathBuf>,
        filter: reserved::ServerFilter,
        output: &mut impl Write,
        colored: bool,
    ) -> Result<(), AppError> {
        codex::workflow::run_list(config, filter, output, colored).await
    }

    async fn export(
        &self,
        args: ExportArgs,
        output: &mut impl Write,
        non_interactive: bool,
        expose_secrets: bool,
    ) -> Result<(), AppError> {
        let filter = args.filter();
        let include_permissions = args.includes_permissions();
        codex::workflow::run_export(
            args.config,
            filter,
            include_permissions,
            output,
            non_interactive,
            expose_secrets,
        )
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
            args.selection,
            args.include_permissions,
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
            args.include_permissions,
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
        codex::diff::run(
            args.file,
            args.config,
            args.selection,
            args.include_permissions,
            output,
            non_interactive,
            colored,
        )
        .await
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
