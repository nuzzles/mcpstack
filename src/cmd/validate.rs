use crate::results::{Action, OperationOutput, Outcome, ServerResult};
use std::fs;
use std::io::Write;
use std::path::PathBuf;

use clap::Args;

use crate::error::AppError;
use crate::schema::Stack;

/// Validate a stack without changing configuration or resolving secrets.
#[derive(Args)]
pub struct Validate {
    /// Path to a YAML stack.
    pub file: PathBuf,
}

impl Validate {
    pub fn run(self, output: &mut OperationOutput<impl Write>) -> Result<(), AppError> {
        let document = fs::read_to_string(self.file).map_err(AppError::StackRead)?;
        let stack = Stack::from_yaml(&document)?;
        let Stack::V1(stack) = stack;
        output.report.stack_schema_version = Some(stack.schema_version);
        output.report.servers = stack
            .servers
            .keys()
            .map(|name| ServerResult {
                name: name.clone(),
                action: Action::Validate,
                outcome: Outcome::Validated,
            })
            .collect();
        writeln!(
            output,
            "Valid stack (schema {}, {} servers).",
            stack.schema_version,
            stack.servers.len()
        )?;
        Ok(())
    }
}
