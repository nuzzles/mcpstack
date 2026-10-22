use clap::Args;
use std::io::Write;
use std::path::PathBuf;

use crate::error::AppError;
use crate::schema::Stack;
use crate::stack_source::StackSource;

/// Validate a stack without changing configuration or resolving secrets.
#[derive(Args)]
pub struct Validate {
    /// YAML stack file path or HTTP(S) URL.
    pub file: PathBuf,
}

impl Validate {
    pub async fn run(self, output: &mut impl Write) -> Result<(), AppError> {
        let source = StackSource::parse(&self.file)?;
        let document = source.read().await?;
        let stack = Stack::from_yaml(&document)?;
        let Stack::V1(stack) = stack;
        if let Some(path) = source.file() {
            writeln!(
                output,
                "Stack file: {}",
                std::path::absolute(path)?.to_string_lossy().escape_debug()
            )?;
        } else {
            writeln!(output, "Stack source: HTTP(S) URL.")?;
        }
        writeln!(
            output,
            "Valid stack (schema {}, {} server(s)).",
            stack.schema_version,
            stack.servers.len()
        )?;
        Ok(())
    }
}
