use crate::error::AppError;
use crate::integrations::{ClientAdapter, StackArgs};
use clap::Args;
use std::io::Write;

/// Approve and apply server additions and replacements.
#[derive(Args)]
pub struct Import {
    /// Run import prompts, then show the diff for approved changes without writing files.
    #[arg(long)]
    dry_run: bool,
    /// Approve all server additions and replacements without prompting.
    #[arg(short = 'y', long)]
    auto_approve: bool,
    #[command(flatten)]
    stack: StackArgs,
}

impl Import {
    pub async fn run(
        self,
        target: &impl ClientAdapter,
        output: &mut impl Write,
        non_interactive: bool,
        colored: bool,
    ) -> Result<(), AppError> {
        target
            .import(
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
