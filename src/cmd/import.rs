use crate::error::AppError;
use crate::integrations::{ClientAdapter, StackArgs};
use clap::Args;
use std::io::Write;

/// Approve and apply server additions and replacements.
#[derive(Args)]
pub struct Import {
    #[command(flatten)]
    selection: crate::integrations::selection::Selection,
    /// Summarize changes and run approval prompts without writing files; use -y for unattended previews.
    #[arg(long)]
    dry_run: bool,
    /// Approve all selected server and permission changes without prompting.
    #[arg(short = 'y', long)]
    auto_approve: bool,
    #[command(flatten)]
    stack: StackArgs,
}

impl Import {
    pub async fn run(
        mut self,
        target: &impl ClientAdapter,
        output: &mut impl Write,
        non_interactive: bool,
        colored: bool,
    ) -> Result<(), AppError> {
        self.stack.selection = self.selection;
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
