use std::io::Write;
use std::path::PathBuf;
use std::{env, fs};

use clap::{Args, Subcommand};

use crate::adapters::codex::detect;
use crate::error::AppError;
use crate::exporters::{codex::ExportError, to_yaml};

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
                tracing::debug!("Exporting Codex MCP configuration");
                let adapter = detect()?;
                if let Some(warning) = adapter.warning() {
                    tracing::warn!("{warning}");
                }
                let path = default_config().ok_or(AppError::ConfigPath)?;
                let document = fs::read_to_string(path).map_err(AppError::ConfigRead)?;
                let stack = adapter.export(&document)?;
                // Prepare the complete result before exposing any content on stdout.
                let yaml = to_yaml(&stack).map_err(|_| ExportError::Stack)?;
                write!(output, "{yaml}")?;
                Ok(())
            }
        }
    }
}

fn default_config() -> Option<PathBuf> {
    if let Some(home) = env::var_os("CODEX_HOME").filter(|value| !value.is_empty()) {
        Some(PathBuf::from(home).join("config.toml"))
    } else {
        directories::BaseDirs::new().map(|dirs| dirs.home_dir().join(".codex/config.toml"))
    }
}
