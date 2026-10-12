use crate::error::AppError;
use crate::integrations::{ClientAdapter, ExportArgs};
use clap::Args;
use std::io::Write;

/// Export client server definitions as a YAML stack on stdout.
#[derive(Args)]
pub struct Export {
    /// Preserve literal credentials in the exported stack.
    #[arg(long)]
    expose_secrets: bool,
    #[command(flatten)]
    args: ExportArgs,
}

impl Export {
    pub async fn run(
        self,
        target: &impl ClientAdapter,
        output: &mut impl Write,
        non_interactive: bool,
    ) -> Result<(), AppError> {
        target
            .export(self.args, output, non_interactive, self.expose_secrets)
            .await
    }
}
