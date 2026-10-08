pub(crate) mod schema;

use crate::cmd::diff::Diff;
use crate::cmd::export::Export;
use crate::cmd::import::Import;
use crate::cmd::schema::Schema;
use crate::cmd::r#use::Use;
use crate::cmd::validate::Validate;
use crate::error::AppError;
use crate::logging::Logging;
use clap::{CommandFactory, Parser, Subcommand};
use std::io::Write;

pub use crate::integrations::EXAMPLES;

/// A Rust CLI for humans and agents to install multiple MCP servers, version-control and switch stacks, and share setups across teams.
#[derive(Parser)]
#[command(
    name = "mcpstack",
    version,
    about = env!("CARGO_PKG_DESCRIPTION"),
    after_help = format!("Examples:\n  {}", EXAMPLES.join("\n  "))
)]
pub struct Cli {
    #[command(flatten)]
    pub logging: Logging,
    /// Disable all prompts; missing masked values fail even during dry-run.
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
    Export(Export),
    Import(Import),
    Diff(Diff),
    Use(Use),
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
                Commands::Export(inner) => inner.run(output, self.non_interactive).await,
                Commands::Import(inner) => {
                    inner
                        .run(output, self.non_interactive, self.logging.output_ansi())
                        .await
                }
                Commands::Use(inner) => {
                    inner
                        .run(output, self.non_interactive, self.logging.output_ansi())
                        .await
                }
                Commands::Diff(inner) => {
                    inner
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
