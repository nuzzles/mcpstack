//! Client selection and adapter dispatch.
mod claude;
mod codex;
mod diff_output;
mod portable;
mod reserved;
mod secrets;

use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Path, PathBuf};

use clap::{Args, Subcommand};
use thiserror::Error;

use crate::cmd::{diff::Diff, export::Export, import::Import, show::Show, r#use::Use};
use crate::error::{AppError, ErrorCode};

fn write_change_summary(
    output: &mut impl Write,
    added: &BTreeSet<String>,
    modified: &BTreeSet<String>,
    deleted: &BTreeSet<String>,
    colored: bool,
) -> std::io::Result<()> {
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
    "mcpstack codex use --help",
    "mcpstack codex use work.yml --dry-run",
    "mcpstack codex show --help",
    "mcpstack codex export --help",
    "mcpstack codex import --help",
    "mcpstack codex diff --help",
    "mcpstack claude --help",
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
    /// Read Claude Code permission rules from this settings file.
    #[arg(long, value_name = "PATH")]
    permissions_config: Option<PathBuf>,
    /// Include every server normally skipped by the export filter.
    #[arg(long)]
    all: bool,
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
    pub(crate) fn config_path(&self) -> Option<&Path> {
        self.config.as_deref()
    }

    fn includes_permissions(&self) -> bool {
        self.all || self.include_permissions
    }
    fn filter(&self) -> reserved::ExportFilter {
        reserved::ExportFilter {
            workspace: self.all || self.allow_workspace,
            claude_in_chrome: self.all || self.allow_claude_in_chrome,
            computer_use: self.all || self.allow_computer_use,
            claude_preview: self.all || self.allow_claude_preview,
            claude_browser: self.all || self.allow_claude_browser,
            node_repl: self.all || self.allow_node_repl,
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
    Show(Show),
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
        Operation::Show(args) => args.run(adapter, output, non_interactive).await,
        Operation::Export(args) => args.run(adapter, output, non_interactive).await,
        Operation::Diff(args) => args.run(adapter, output, non_interactive, colored).await,
    }
}

pub trait ClientAdapter {
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
