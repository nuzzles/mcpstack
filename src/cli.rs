pub(crate) mod schema;

use crate::cmd::export::Export;
use crate::cmd::import::Import;
use crate::cmd::schema::Schema;
use crate::cmd::validate::Validate;
use crate::error::AppError;
use crate::logging::Logging;
use clap::{CommandFactory, Parser, Subcommand};
use std::io::Write;

pub const EXAMPLES: &[&str] = &[
    "mcpstack --schema",
    "mcpstack --help",
    "mcpstack validate --help",
    "mcpstack export codex --help",
    "mcpstack import codex --help",
];

/// Install MCP servers, version-control stacks, and share setups across teams.
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
    /// Disable prompts; import requires --auto-approve or --dry-run.
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
}

impl Cli {
    pub fn run(self, output: &mut impl Write) -> Result<(), AppError> {
        if self.schema && self.command.is_some() {
            return Err(AppError::Arguments);
        }
        if self.schema {
            Schema.run(output)?;
            Ok(())
        } else if let Some(command) = self.command {
            match command {
                Commands::Validate(inner) => inner.run(output),
                Commands::Export(inner) => inner.run(output, self.non_interactive),
                Commands::Import(inner) => inner.run(output, self.non_interactive),
            }
        } else {
            Self::command().write_long_help(output)?;
            Ok(())
        }
    }
}
