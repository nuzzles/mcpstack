use crate::error::AppError;
use crate::integrations::{ClientAdapter, StackArgs};
use clap::Args;
use std::io::Write;

/// Switch stacks, replacing ALL MCP servers while preserving other client settings.
#[derive(Args)]
pub struct Use<S: Args + Into<StackArgs> = StackArgs> {
    #[command(flatten)]
    stack: S,
    /// Summarize additions, replacements, and removals without approval prompts or writes.
    #[arg(long)]
    dry_run: bool,
    /// Approve replacing the entire server set and create a numbered backup.
    #[arg(short = 'y', long)]
    auto_approve: bool,
}

impl<S: Args + Into<StackArgs>> Use<S> {
    pub fn is_dry_run(&self) -> bool {
        self.dry_run
    }

    pub async fn run(
        self,
        target: &impl ClientAdapter,
        output: &mut impl Write,
        non_interactive: bool,
        colored: bool,
    ) -> Result<(), AppError> {
        target
            .use_stack(
                self.stack.into(),
                output,
                non_interactive,
                colored,
                self.dry_run,
                self.auto_approve,
            )
            .await
    }
}
