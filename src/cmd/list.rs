use clap::Args;
use std::io::Write;
use std::path::PathBuf;

use crate::error::AppError;
use crate::integrations::ClientAdapter;

/// List configured MCP servers without starting them or checking connections.
#[derive(Args)]
pub struct List {
    /// Read this client config file instead of the default configuration.
    #[arg(long, value_name = "PATH")]
    config: Option<PathBuf>,
}

impl List {
    pub async fn run(
        self,
        target: &impl ClientAdapter,
        output: &mut impl Write,
    ) -> Result<(), AppError> {
        target.list(self.config, output).await
    }
}
