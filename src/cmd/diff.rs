use crate::error::AppError;
use crate::integrations::StackClient;
use crate::results::OperationOutput;
use clap::Args;
use std::io::Write;

/// Show a redacted diff of a stack against client configuration without writing files.
#[derive(Args)]
pub struct Diff {
    #[command(subcommand)]
    client: StackClient,
}

impl Diff {
    pub fn run(
        self,
        output: &mut OperationOutput<impl Write>,
        non_interactive: bool,
        colored: bool,
    ) -> Result<(), AppError> {
        self.client.diff(output, non_interactive, colored)
    }
}
