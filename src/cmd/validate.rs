use std::fs;
use std::io::Write;
use std::path::PathBuf;

use clap::Args;

use crate::error::AppError;
use crate::schema::{Server, Stack};

/// Validate a stack without changing configuration or resolving secrets.
#[derive(Args)]
pub struct Validate {
    /// Path to a YAML stack.
    pub file: PathBuf,
}

impl Validate {
    pub fn run(self, output: &mut impl Write) -> Result<(), AppError> {
        let document = fs::read_to_string(self.file).map_err(AppError::StackRead)?;
        let stack = Stack::from_yaml(&document)?;
        let Stack::V1(stack) = stack;
        writeln!(
            output,
            "Valid stack (schema {}, {} servers).",
            stack.schema_version,
            stack.servers.len()
        )?;
        let native_count = stack
            .servers
            .values()
            .filter(|server| matches!(server, Server::ClientSpecific { .. }))
            .count();
        if native_count != 0 {
            writeln!(
                output,
                "{native_count} client-specific definitions require client adapter validation before use."
            )?;
        }
        Ok(())
    }
}
