use crate::error::AppError;
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
    /// Include the otherwise skipped computer-use server.
    #[arg(long)]
    allow_computer_use: bool,
    /// Include the otherwise skipped node_repl server.
    #[arg(long)]
    allow_node_repl: bool,
}

impl CodexShow {
    pub(crate) fn into_show(mut self) -> Show {
        self.show
            .args
            .for_codex(self.allow_computer_use, self.allow_node_repl);
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
}

impl ClaudeShow {
    pub(crate) fn into_show(mut self) -> Show {
        self.show.args.for_claude(
            self.permissions_config,
            self.allow_workspace,
            self.allow_claude_in_chrome,
            self.allow_computer_use,
            self.allow_claude_preview,
            self.allow_claude_browser,
        );
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
