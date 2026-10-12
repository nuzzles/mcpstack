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
    export::{ClaudeExport, CodexExport, DesktopExport},
    import::Import,
    list::List,
    show::{ClaudeShow, CodexShow, DesktopShow},
    r#use::Use,
};
use crate::error::{AppError, ErrorCode};

fn write_dry_run(output: &mut impl Write) -> std::io::Result<()> {
    writeln!(output, "Dry run; no configuration files were changed.")
}

fn write_no_changes(output: &mut impl Write, target: &Path, dry_run: bool) -> std::io::Result<()> {
    writeln!(
        output,
        "Target config: {}",
        std::path::absolute(target)?
            .to_string_lossy()
            .escape_debug()
    )?;
    writeln!(
        output,
        "No changes; requested configuration is already in place."
    )?;
    if dry_run {
        write_dry_run(output)?;
    }
    Ok(())
}

fn write_import_result(
    output: &mut impl Write,
    count: usize,
    permissions_changed: bool,
    dry_run: bool,
) -> std::io::Result<()> {
    if count > 0 {
        let verb = if dry_run { "Would import" } else { "Imported" };
        writeln!(
            output,
            "{verb} {count} MCP server(s); identical entries were unchanged."
        )?;
    }
    if permissions_changed {
        let verb = if dry_run { "Would update" } else { "Updated" };
        writeln!(output, "{verb} MCP permission rules.")?;
    }
    if dry_run {
        write_dry_run(output)?;
    }
    Ok(())
}

fn write_switch_plan(output: &mut impl Write, count: usize) -> std::io::Result<()> {
    writeln!(
        output,
        "Stack switch plan: {count} MCP server(s); servers absent from the stack will be removed."
    )
}

fn write_switch_result(output: &mut impl Write, count: usize) -> std::io::Result<()> {
    writeln!(
        output,
        "Using stack with {count} MCP server(s); all other MCP servers were removed."
    )
}

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
        std::path::absolute(target)?
            .to_string_lossy()
            .escape_debug()
    )?;
    writeln!(output, "MCP server changes:")?;
    let entries = [added, modified, deleted]
        .map(|names| names.iter().map(|name| format!("{name:?}")).collect());
    write_change_rows(output, &entries, colored)
}

fn write_change_rows(
    output: &mut impl Write,
    changes: &[Vec<String>; 3],
    colored: bool,
) -> std::io::Result<()> {
    for ((marker, label, color), entries) in [
        ("+", "Added", 32),
        ("~", "Modified", 33),
        ("-", "Deleted", 31),
    ]
    .into_iter()
    .zip(changes)
    {
        let changed = !entries.is_empty();
        let entries = if entries.is_empty() {
            "none".to_owned()
        } else {
            entries.join(", ")
        };
        if colored && changed {
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
    "mcpstack claude-desktop --help",
    "mcpstack claude-desktop use --help",
];

#[derive(Args)]
pub struct ExportArgs {
    /// Read this client config file instead of the default configuration.
    #[arg(long, value_name = "PATH")]
    pub(crate) config: Option<PathBuf>,
    /// Include per-server MCP permissions and tool filters; --all does not include them.
    #[arg(long)]
    pub(crate) include_permissions: bool,
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
        self.include_permissions
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
    /// Apply or compare permissions and tool filters; otherwise preserve the target policy.
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
    /// Manage local MCP servers in Claude Desktop configuration.
    ClaudeDesktop(DesktopCommands),
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
    #[command(
        after_help = "Examples:\n  mcpstack codex use work.yml\n  mcpstack codex use personal.yml --dry-run\n  mcpstack codex use work.yml --config config.toml -y"
    )]
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
    #[command(
        after_help = "Examples:\n  mcpstack claude use work.yml\n  mcpstack claude use personal.yml --dry-run\n  mcpstack claude use work.yml --config claude.json -y"
    )]
    Use(Use),
    Import(Import),
    Show(ClaudeShow),
    Export(ClaudeExport),
    Diff(Diff),
}

/// Desktop has no portable permission settings in its local MCP config file.
#[derive(Args)]
pub struct DesktopStackArgs {
    /// Stack file to compare, import, or use.
    #[arg(value_name = "FILE")]
    file: PathBuf,
    /// Use this Claude Desktop config file instead of the default configuration.
    #[arg(long, value_name = "PATH")]
    config: Option<PathBuf>,
}

impl From<DesktopStackArgs> for StackArgs {
    fn from(args: DesktopStackArgs) -> Self {
        Self {
            file: args.file,
            config: args.config,
            selection: Default::default(),
            include_permissions: false,
            permissions_config: None,
        }
    }
}

#[derive(Args)]
#[command(arg_required_else_help = false)]
pub struct DesktopCommands {
    #[command(subcommand)]
    operation: DesktopOperation,
}

#[derive(Subcommand)]
enum DesktopOperation {
    List(List<reserved::DesktopFilterArgs>),
    #[command(
        after_help = "Examples:\n  mcpstack claude-desktop use work.yml --dry-run\n  mcpstack claude-desktop use work.yml --config claude_desktop_config.json -y\n\nRestart Claude Desktop after applying changes."
    )]
    Use(Use<DesktopStackArgs>),
    Import(Import<DesktopStackArgs>),
    Show(DesktopShow),
    Export(DesktopExport),
    Diff(Diff<DesktopStackArgs>),
}

impl Client {
    // Read-only discovery and previews must not create even an update cache.
    pub fn allows_update_check(&self) -> bool {
        match self {
            Self::Codex(commands) => match &commands.operation {
                CodexOperation::Export(_) => true,
                CodexOperation::Import(args) => !args.is_dry_run(),
                CodexOperation::Use(args) => !args.is_dry_run(),
                _ => false,
            },
            Self::ClaudeDesktop(commands) => match &commands.operation {
                DesktopOperation::Export(_) => true,
                DesktopOperation::Import(args) => !args.is_dry_run(),
                DesktopOperation::Use(args) => !args.is_dry_run(),
                _ => false,
            },
            Self::Claude(commands) => match &commands.operation {
                ClaudeOperation::Export(_) => true,
                ClaudeOperation::Import(args) => !args.is_dry_run(),
                ClaudeOperation::Use(args) => !args.is_dry_run(),
                _ => false,
            },
        }
    }

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
            Self::ClaudeDesktop(commands) => match commands.operation {
                DesktopOperation::List(args) => args.run(&Claude::Desktop, output, colored).await,
                DesktopOperation::Use(args) => {
                    args.run(&Claude::Desktop, output, non_interactive, colored)
                        .await
                }
                DesktopOperation::Import(args) => {
                    args.run(&Claude::Desktop, output, non_interactive, colored)
                        .await
                }
                DesktopOperation::Show(args) => {
                    args.run(&Claude::Desktop, output, non_interactive).await
                }
                DesktopOperation::Export(args) => {
                    args.run(&Claude::Desktop, output, non_interactive).await
                }
                DesktopOperation::Diff(args) => {
                    args.run(&Claude::Desktop, output, non_interactive, colored)
                        .await
                }
            },
            Self::Claude(commands) => match commands.operation {
                ClaudeOperation::List(args) => args.run(&Claude::Code, output, colored).await,
                ClaudeOperation::Use(args) => {
                    args.run(&Claude::Code, output, non_interactive, colored)
                        .await
                }
                ClaudeOperation::Import(args) => {
                    args.run(&Claude::Code, output, non_interactive, colored)
                        .await
                }
                ClaudeOperation::Show(args) => {
                    args.run(&Claude::Code, output, non_interactive).await
                }
                ClaudeOperation::Export(args) => {
                    args.run(&Claude::Code, output, non_interactive).await
                }
                ClaudeOperation::Diff(args) => {
                    args.run(&Claude::Code, output, non_interactive, colored)
                        .await
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Claude {
    Code,
    Desktop,
}

impl std::fmt::Display for Claude {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Code => "Claude Code",
            Self::Desktop => "Claude Desktop",
        })
    }
}

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
