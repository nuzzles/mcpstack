use crate::error::AppError;
use crate::integrations::{ClientAdapter, StackArgs};
use clap::Args;
use std::io::Write;

/// Show a redacted diff of a stack against client configuration without writing files.
#[derive(Args)]
pub struct Diff {
    #[command(flatten)]
    stack: StackArgs,
}

impl Diff {
    pub async fn run(
        self,
        target: &impl ClientAdapter,
        output: &mut impl Write,
        non_interactive: bool,
        colored: bool,
    ) -> Result<(), AppError> {
        target
            .diff(self.stack, output, non_interactive, colored)
            .await
    }
}
