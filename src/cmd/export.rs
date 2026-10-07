use crate::error::AppError;
use crate::integrations::ExportClient;
use clap::Args;
use std::io::Write;

/// Export client server definitions as a YAML stack on stdout.
#[derive(Args)]
pub struct Export {
    /// Preserve literal credentials in the exported stack.
    #[arg(long, global = true)]
    expose_secrets: bool,
    #[command(subcommand)]
    client: ExportClient,
}

impl Export {
    pub fn run(self, output: &mut impl Write, non_interactive: bool) -> Result<(), AppError> {
        self.client
            .run(output, non_interactive, self.expose_secrets)
    }
}
