use crate::cmd::show::{ClaudeShow, CodexShow, Show};
use crate::error::AppError;
use crate::integrations::ClientAdapter;
use crate::io;
use clap::Args;
use dialoguer::Confirm;
use std::io::{IsTerminal, Write, stderr, stdin};
use std::path::PathBuf;

/// Export client server definitions as a YAML stack to a file.
struct Export {
    file: PathBuf,
    overwrite: bool,
    show: Show,
}

#[derive(Args)]
struct Destination {
    /// Destination YAML stack file.
    #[arg(value_name = "FILE")]
    file: PathBuf,
    /// Replace an existing stack file without prompting.
    #[arg(long)]
    overwrite: bool,
}

/// Export Codex MCP servers to a YAML stack file.
#[derive(Args)]
pub struct CodexExport {
    #[command(flatten)]
    destination: Destination,
    #[command(flatten)]
    show: CodexShow,
}

impl CodexExport {
    pub async fn run(
        self,
        target: &impl ClientAdapter,
        output: &mut impl Write,
        non_interactive: bool,
    ) -> Result<(), AppError> {
        Export {
            file: self.destination.file,
            overwrite: self.destination.overwrite,
            show: self.show.into_show(),
        }
        .run(target, output, non_interactive)
        .await
    }
}

/// Export Claude Code MCP servers to a YAML stack file.
#[derive(Args)]
pub struct ClaudeExport {
    #[command(flatten)]
    destination: Destination,
    #[command(flatten)]
    show: ClaudeShow,
}

impl ClaudeExport {
    pub async fn run(
        self,
        target: &impl ClientAdapter,
        output: &mut impl Write,
        non_interactive: bool,
    ) -> Result<(), AppError> {
        Export {
            file: self.destination.file,
            overwrite: self.destination.overwrite,
            show: self.show.into_show(),
        }
        .run(target, output, non_interactive)
        .await
    }
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
        // Keep the same snapshot through approval and generation so a newly
        // created or changed destination cannot be silently overwritten.
        let destination = io::Snapshot::read(&self.file)
            .await
            .map_err(AppError::StackWrite)?;
        if destination.exists() && !self.overwrite {
            if non_interactive || !stdin().is_terminal() || !stderr().is_terminal() {
                return Err(AppError::ExportOverwriteRequired);
            }
            if !Confirm::new()
                .with_prompt(format!("Overwrite existing stack file {:?}?", self.file))
                .default(false)
                .report(false)
                .interact()
                .map_err(|_| AppError::ExportApprovalCancelled)?
            {
                writeln!(
                    output,
                    "Export cancelled; existing stack file was not changed."
                )?;
                return Ok(());
            }
        }
        let mut yaml = Vec::new();
        self.show.run(target, &mut yaml, non_interactive).await?;
        destination
            .replace(&yaml)
            .await
            .map_err(AppError::StackWrite)?;
        writeln!(
            output,
            "Exported stack to {}.",
            std::path::absolute(&self.file)?
                .to_string_lossy()
                .escape_debug()
        )?;
        Ok(())
    }
}

pub(crate) async fn run_with(
    file: PathBuf,
    overwrite: bool,
    show: Show,
    target: &impl ClientAdapter,
    output: &mut impl Write,
    non_interactive: bool,
) -> Result<(), AppError> {
    Export {
        file,
        overwrite,
        show,
    }
    .run(target, output, non_interactive)
    .await
}
