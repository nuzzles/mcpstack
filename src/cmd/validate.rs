use clap::Args;
use std::io::Write;

use crate::error::AppError;
use crate::schema::Stack;
use crate::stack_source::StackSource;

/// Validate a stack without changing configuration or resolving secrets.
#[derive(Args)]
pub struct Validate {
    /// YAML stack file path or HTTP(S) URL.
    #[arg(value_name = "SOURCE")]
    pub file: StackSource,
}

impl Validate {
    pub async fn run(self, output: &mut impl Write) -> Result<(), AppError> {
        let source = self.file;
        let document = source.read().await?;
        let stack = Stack::from_yaml(&document)?;
        let Stack::V1(stack) = stack;
        match &source {
            StackSource::File(local) => writeln!(
                output,
                "Stack file: {}",
                std::path::absolute(&local.path)?
                    .to_string_lossy()
                    .escape_debug()
            )?,
            StackSource::Url(url) => {
                writeln!(output, "Stack URL: {}", url.as_str().escape_debug())?
            }
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
