use crate::error::AppError;
use crate::integrations::reserved::{
    ClaudeFilterArgs, CodexFilterArgs, DesktopFilterArgs, FilterArgs,
};
use crate::integrations::{ClientAdapter, ExportArgs};
use clap::Args;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;

/// Show client server definitions as a YAML stack on stdout.
#[derive(Args)]
pub struct Show {
    /// Preserve literal credentials in the stack.
    #[arg(long)]
    expose_secrets: bool,
    #[command(flatten)]
    args: ExportArgs,
}

impl Show {
    pub(crate) fn config_path(&self) -> Option<&Path> {
        self.args.config_path()
    }

    pub async fn run(
        self,
        target: &impl ClientAdapter,
        output: &mut impl Write,
        non_interactive: bool,
    ) -> Result<(), AppError> {
        target
            .show(self.args, output, non_interactive, self.expose_secrets)
            .await
    }
}

/// Show Codex MCP servers as a YAML stack.
#[derive(Args)]
pub struct CodexShow {
    #[command(flatten)]
    show: Show,
    #[command(flatten)]
    filter: CodexFilterArgs,
}

impl CodexShow {
    pub(crate) fn into_show(mut self) -> Show {
        self.show.args.server_filter = self.filter.filter();
        self.show
    }

    pub async fn run(
        self,
        target: &impl ClientAdapter,
        output: &mut impl Write,
        non_interactive: bool,
    ) -> Result<(), AppError> {
        self.into_show().run(target, output, non_interactive).await
    }
}

/// Show Claude Code MCP servers as a YAML stack.
#[derive(Args)]
pub struct ClaudeShow {
    #[command(flatten)]
    show: Show,
    /// Read Claude Code permission rules from this settings file.
    #[arg(long, value_name = "PATH")]
    permissions_config: Option<PathBuf>,
    #[command(flatten)]
    filter: ClaudeFilterArgs,
}

impl ClaudeShow {
    pub(crate) fn into_show(mut self) -> Show {
        self.show.args.permissions_config = self.permissions_config;
        self.show.args.server_filter = self.filter.filter();
        self.show
    }

    pub async fn run(
        self,
        target: &impl ClientAdapter,
        output: &mut impl Write,
        non_interactive: bool,
    ) -> Result<(), AppError> {
        self.into_show().run(target, output, non_interactive).await
    }
}

/// Show Claude Desktop local MCP servers as a YAML stack.
#[derive(Args)]
pub struct DesktopShow {
    /// Read this Claude Desktop config file instead of the default configuration.
    #[arg(long, value_name = "PATH")]
    config: Option<PathBuf>,
    /// Preserve literal credentials in the stack.
    #[arg(long)]
    expose_secrets: bool,
    #[command(flatten)]
    filter: DesktopFilterArgs,
}

impl DesktopShow {
    pub(crate) fn into_show(self) -> Show {
        Show {
            expose_secrets: self.expose_secrets,
            args: ExportArgs {
                config: self.config,
                include_permissions: false,
                permissions_config: None,
                server_filter: self.filter.filter(),
            },
        }
    }

    pub async fn run(
        self,
        target: &impl ClientAdapter,
        output: &mut impl Write,
        non_interactive: bool,
    ) -> Result<(), AppError> {
        self.into_show().run(target, output, non_interactive).await
    }
}
