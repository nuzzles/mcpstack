use clap::Args;
use std::io::Write;
use std::path::PathBuf;

use crate::error::AppError;
use crate::integrations::ClientAdapter;
use crate::integrations::reserved::FilterArgs;

/// List configured MCP servers without starting them or checking connections.
#[derive(Args)]
pub struct List<F: Args + FilterArgs> {
    /// Read this client config file instead of the default configuration.
    #[arg(long, value_name = "PATH")]
    config: Option<PathBuf>,
    #[command(flatten)]
    filter: F,
}

impl<F: Args + FilterArgs> List<F> {
    pub async fn run(
        self,
        target: &impl ClientAdapter,
        output: &mut impl Write,
    ) -> Result<(), AppError> {
        target.list(self.config, self.filter.filter(), output).await
    }
}
