use crate::error::AppError;
use crate::integrations::{ClientAdapter, StackArgs};
use clap::Args;
use std::io::Write;

/// Show a redacted diff of a stack against client configuration without writing files.
#[derive(Args)]
pub struct Diff<S: Args + Into<StackArgs> = StackArgs> {
    #[command(flatten)]
    selection: crate::integrations::selection::Selection,
    #[command(flatten)]
    stack: S,
}

impl<S: Args + Into<StackArgs>> Diff<S> {
    pub async fn run(
        self,
        target: &impl ClientAdapter,
        output: &mut impl Write,
        non_interactive: bool,
        colored: bool,
    ) -> Result<(), AppError> {
        let mut stack = self.stack.into();
        stack.selection = self.selection;
        target.diff(stack, output, non_interactive, colored).await
    }
}
