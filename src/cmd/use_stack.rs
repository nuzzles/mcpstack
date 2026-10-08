use crate::error::AppError;
use crate::integrations::{StackArgs, UseClient};
use clap::Args;
use std::io::Write;

/// Switch stacks, replacing ALL MCP servers while preserving other client settings.
#[derive(Args)]
#[command(
    after_help = "Examples:\n  mcpstack use work.yml\n  mcpstack use personal.yml --dry-run\n  mcpstack use work.yml --client codex --config config.toml -y"
)]
pub struct Use {
    #[command(flatten)]
    stack: StackArgs,
    /// Target client configuration to switch.
    #[arg(long, value_enum, default_value = "codex")]
    client: UseClient,
    /// Preview all additions, replacements, and removals without writing files.
    #[arg(long)]
    dry_run: bool,
    /// Approve replacing the entire server set and create a numbered backup.
    #[arg(short = 'y', long)]
    auto_approve: bool,
}

impl Use {
    pub async fn run(
        self,
        output: &mut impl Write,
        non_interactive: bool,
        colored: bool,
    ) -> Result<(), AppError> {
        self.client
            .run(
                self.stack,
                output,
                non_interactive,
                colored,
                self.dry_run,
                self.auto_approve,
            )
            .await
    }
}
