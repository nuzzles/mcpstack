use std::io::Write;
use std::path::PathBuf;
use tokio::fs;

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
    pub async fn run(self, output: &mut impl Write) -> Result<(), AppError> {
        let document = fs::read_to_string(self.file)
            .await
            .map_err(AppError::StackRead)?;
        let stack = Stack::from_yaml(&document)?;
        let Stack::V1(stack) = stack;
        writeln!(
            output,
            "Valid stack (schema {}, {} servers).",
            stack.schema_version,
            stack.servers.len()
        )?;
        Ok(())
    }
}
