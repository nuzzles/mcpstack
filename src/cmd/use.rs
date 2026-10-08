use crate::error::AppError;
use crate::integrations::{StackArgs, Target};
use clap::Args;
use std::io::Write;

/// Switch stacks, replacing ALL MCP servers while preserving other client settings.
#[derive(Args)]
#[command(
    after_help = "Examples:\n  mcpstack codex use work.yml\n  mcpstack codex use personal.yml --dry-run\n  mcpstack codex use work.yml --config config.toml -y"
)]
pub struct Use {
    #[command(flatten)]
    stack: StackArgs,
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
        target: Target,
        output: &mut impl Write,
        non_interactive: bool,
        colored: bool,
    ) -> Result<(), AppError> {
        target
            .use_stack(
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
