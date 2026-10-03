use std::io::{self, Write};

use clap::CommandFactory;

use crate::cli::{Cli, schema};

/// Emit the CLI interface generated from its argument definitions.
pub struct Schema;

impl Schema {
    // FIXME: Use Clap's JSON export/Serde integration once available, retaining our output handling.
    // https://github.com/clap-rs/clap/issues/918
    // https://github.com/clap-rs/clap/issues/6299
    pub fn run(self, output: &mut impl Write) -> io::Result<()> {
        let description = schema::describe(Cli::command());
        serde_json::to_writer_pretty(&mut *output, &description)?;
        writeln!(output)
    }
}
