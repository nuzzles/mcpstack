use crate::cmd::show::Show;
use crate::error::AppError;
use crate::integrations::ClientAdapter;
use crate::io;
use clap::Args;
use std::io::Write;
use std::path::PathBuf;

/// Export client server definitions as a YAML stack to a file.
#[derive(Args)]
pub struct Export {
    /// Destination YAML stack file.
    #[arg(value_name = "FILE")]
    file: PathBuf,
    #[command(flatten)]
    show: Show,
}

impl Export {
    pub async fn run(
        self,
        target: &impl ClientAdapter,
        output: &mut impl Write,
        non_interactive: bool,
    ) -> Result<(), AppError> {
        let yaml_file = self
            .file
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|extension| {
                extension.eq_ignore_ascii_case("yml") || extension.eq_ignore_ascii_case("yaml")
            });
        if !yaml_file {
            return Err(AppError::ExportPath);
        }
        if let Some(source) = self.show.config_path()
            && let (Ok(source), Ok(destination)) = (
                tokio::fs::canonicalize(source).await,
                tokio::fs::canonicalize(&self.file).await,
            )
            && source == destination
        {
            return Err(AppError::ExportSource);
        }
        let mut yaml = Vec::new();
        self.show.run(target, &mut yaml, non_interactive).await?;
        io::Snapshot::read(&self.file)
            .await
            .map_err(AppError::StackWrite)?
            .replace(&yaml)
            .await
            .map_err(AppError::StackWrite)?;
        writeln!(output, "Exported stack to {}.", self.file.display())?;
        Ok(())
    }
}
