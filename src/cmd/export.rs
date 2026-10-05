use std::io::Write;
use std::path::PathBuf;

use clap::{Args, Subcommand};

use crate::error::AppError;
use crate::exporters::codex;

/// Export client server definitions as a YAML stack on stdout.
#[derive(Args)]
pub struct Export {
    #[command(subcommand)]
    client: Client,
}

#[derive(Subcommand)]
enum Client {
    Codex,
}

impl Export {
    pub fn run(self, output: &mut impl Write) -> Result<(), AppError> {
        match self.client {
            Client::Codex => {
                let adapter = crate::adapters::codex::detect()?;
                if let Some(warning) = adapter.warning() {
                    writeln!(std::io::stderr().lock(), "{warning}")?;
                }
                let path = default_config().ok_or(AppError::ConfigPath)?;
                let document = std::fs::read_to_string(path).map_err(AppError::ConfigRead)?;
                let stack = adapter.export(&document)?;
                // Prepare the complete result before exposing any content on stdout.
                let yaml =
                    crate::exporters::to_yaml(&stack).map_err(|_| codex::ExportError::Stack)?;
                write!(output, "{yaml}")?;
                Ok(())
            }
        }
    }
}

fn default_config() -> Option<PathBuf> {
    if let Some(home) = std::env::var_os("CODEX_HOME").filter(|value| !value.is_empty()) {
        Some(PathBuf::from(home).join("config.toml"))
    } else {
        directories::BaseDirs::new().map(|dirs| dirs.home_dir().join(".codex/config.toml"))
    }
}
