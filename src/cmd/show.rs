use crate::error::AppError;
use crate::integrations::{ClientAdapter, ExportArgs};
use clap::Args;
use std::io::Write;
use std::path::Path;

/// Show client server definitions as a YAML stack on stdout.
#[derive(Args)]
pub struct Show {
    /// Preserve literal credentials in the stack.
    #[arg(long)]
    expose_secrets: bool,
    #[command(flatten)]
    args: ExportArgs,
}

impl Show {
    pub(crate) fn config_path(&self) -> Option<&Path> {
        self.args.config_path()
    }

    pub async fn run(
        self,
        target: &impl ClientAdapter,
        output: &mut impl Write,
        non_interactive: bool,
    ) -> Result<(), AppError> {
        target
            .show(self.args, output, non_interactive, self.expose_secrets)
            .await
    }
}
