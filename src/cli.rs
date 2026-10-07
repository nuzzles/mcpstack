pub(crate) mod schema;

use crate::cmd::diff::Diff;
use crate::cmd::export::Export;
use crate::cmd::import::Import;
use crate::cmd::schema::Schema;
use crate::cmd::validate::Validate;
use crate::error::AppError;
use crate::logging::Logging;
use crate::results::{Operation, OperationOutput};
use clap::{CommandFactory, Parser, Subcommand};
use std::io::Write;

pub use crate::integrations::EXAMPLES;

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
    /// Disable all prompts; missing masked values fail even during dry-run.
    #[arg(long, global = true)]
    non_interactive: bool,
    /// Print the complete CLI interface as versioned JSON.
    #[arg(long, conflicts_with = "json")]
    schema: bool,
    /// Emit a versioned JSON result for validate, diff, or import; prompts are unchanged.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    Validate(Validate),
    Export(Export),
    Import(Import),
    Diff(Diff),
}

impl Cli {
    pub fn run(self, output: &mut impl Write) -> Result<(), AppError> {
        let operation = self.command.as_ref().map(|command| match command {
            Commands::Validate(_) => Operation::Validate,
            Commands::Diff(_) => Operation::Diff,
            Commands::Import(_) => Operation::Import,
            Commands::Export(_) => Operation::Export,
        });
        let mut output = OperationOutput::new(output, self.json, operation);
        let result = (|| {
            self.logging.init()?;
            if (self.schema && self.command.is_some())
                || (self.json
                    && (self.schema
                        || self.command.is_none()
                        || matches!(self.command, Some(Commands::Export(_)))))
            {
                return Err(AppError::Arguments);
            }
            if self.schema {
                Ok(Schema.run(&mut output)?)
            } else if let Some(command) = self.command {
                match command {
                    Commands::Validate(inner) => inner.run(&mut output),
                    Commands::Export(inner) => inner.run(&mut output, self.non_interactive),
                    Commands::Import(inner) => inner.run(
                        &mut output,
                        self.non_interactive,
                        self.logging.output_ansi(),
                    ),
                    Commands::Diff(inner) => inner.run(
                        &mut output,
                        self.non_interactive,
                        self.logging.output_ansi(),
                    ),
                }
            } else {
                Self::command().write_long_help(&mut output)?;
                Ok(())
            }
        })();
        output.finish(&result)?;
        result
    }
}
