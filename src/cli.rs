pub(crate) mod schema;

use crate::cmd::schema::Schema;
use clap::Parser;
use std::io::{self, Write};

pub const EXAMPLES: &[&str] = &["mcpstack --schema", "mcpstack --help"];

/// Install MCP servers, version-control stacks, and share setups across teams.
#[derive(Parser)]
#[command(
    name = "mcpstack",
    version,
    about = env!("CARGO_PKG_DESCRIPTION"),
    after_help = format!("Examples:\n  {}", EXAMPLES.join("\n  "))
)]
pub struct Cli {
    /// Print the complete CLI interface as versioned JSON.
    #[arg(long)]
    schema: bool,
}

impl Cli {
    pub fn run(self, output: &mut impl Write) -> io::Result<()> {
        if self.schema {
            Schema.run(output)
        } else {
            writeln!(
                output,
                "mcpstack {} — work in progress",
                env!("CARGO_PKG_VERSION")
            )
        }
    }
}
