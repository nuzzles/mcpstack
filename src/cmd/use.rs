use crate::error::AppError;
use crate::integrations::{ClientAdapter, StackArgs};
use clap::Args;
use std::io::Write;

/// Switch stacks, replacing ALL MCP servers while preserving other client settings.
#[derive(Args)]
pub struct Use {
    #[command(flatten)]
    stack: StackArgs,
    /// Summarize additions, replacements, and removals without approval prompts or writes.
    #[arg(long)]
    dry_run: bool,
    /// Approve replacing the entire server set and create a numbered backup.
    #[arg(short = 'y', long)]
    auto_approve: bool,
}

impl Use {
    pub async fn run(
        self,
        target: &impl ClientAdapter,
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
