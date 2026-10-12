mod diagnostics;
pub(crate) mod schema;

use crate::cmd::schema::Schema;
use crate::cmd::validate::Validate;
use crate::error::AppError;
use crate::integrations::Client;
use crate::logging::Logging;
use clap::{CommandFactory, Parser, Subcommand};
use std::io::Write;

pub use crate::integrations::EXAMPLES;

/// Manage, share, and switch MCP server configurations across clients.
#[derive(Parser)]
#[command(
    name = "mcpstack",
    version,
    styles = diagnostics::styles(),
    about = env!("CARGO_PKG_DESCRIPTION"),
    after_help = format!("Examples:\n  {}", EXAMPLES.join("\n  "))
)]
pub struct Cli {
    #[command(flatten)]
    pub logging: Logging,
    /// Disable interactive prompts.
    #[arg(long, global = true)]
    non_interactive: bool,
    /// Print the complete CLI interface as versioned JSON.
    #[arg(long)]
    schema: bool,
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    Validate(Validate),
    #[command(flatten)]
    Client(Client),
}

impl Cli {
    pub async fn run(self, output: &mut impl Write) -> Result<(), AppError> {
        if self.schema && self.command.is_some() {
            return Err(AppError::Arguments);
        }
        if self.schema {
            Schema.run(output)?;
            Ok(())
        } else if let Some(command) = self.command {
            match command {
                Commands::Validate(inner) => inner.run(output).await,
                Commands::Client(client) => {
                    client
                        .run(output, self.non_interactive, self.logging.output_ansi())
                        .await
                }
            }
        } else {
            Self::command().write_long_help(output)?;
            Ok(())
        }
    }
}
