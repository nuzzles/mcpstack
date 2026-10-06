use crate::error::AppError;
use crate::integrations::StackClient;
use clap::Args;
use std::io::Write;

/// Approve and apply server additions and replacements.
#[derive(Args)]
pub struct Import {
    /// Run import prompts, then show the diff for approved changes without writing files.
    #[arg(long, global = true)]
    dry_run: bool,
    /// Approve all server additions and replacements without prompting.
    #[arg(short = 'y', long, global = true)]
    auto_approve: bool,
    #[command(subcommand)]
    client: StackClient,
}

impl Import {
    pub fn run(
        self,
        output: &mut impl Write,
        non_interactive: bool,
        colored: bool,
    ) -> Result<(), AppError> {
        self.client.import(
            output,
            non_interactive,
            colored,
            self.dry_run,
            self.auto_approve,
        )
    }
}
