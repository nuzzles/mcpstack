use std::io::{IsTerminal, Write, stderr, stdin};
use std::path::PathBuf;
use std::{env, fs};

use clap::{Args, Subcommand};
use dialoguer::Select;

use crate::error::AppError;
use crate::exporters::{
    codex::{ExportError, export, export_with_decisions},
    to_yaml,
};

/// Export client server definitions as a YAML stack on stdout.
#[derive(Args)]
pub struct Export {
    /// Preserve literal credentials in the exported stack.
    #[arg(long, global = true)]
    expose_secrets: bool,
    #[command(subcommand)]
    client: Client,
}

#[derive(Subcommand)]
enum Client {
    Codex {
        /// Read this Codex TOML file instead of the default configuration.
        #[arg(long, value_name = "PATH")]
        config: Option<PathBuf>,
    },
}

impl Export {
    pub fn run(self, output: &mut impl Write, non_interactive: bool) -> Result<(), AppError> {
        match self.client {
            Client::Codex { config } => {
                tracing::debug!("Exporting Codex MCP configuration");
                let path = config.or_else(default_config).ok_or(AppError::ConfigPath)?;
                let document = fs::read_to_string(path).map_err(AppError::ConfigRead)?;
                let stack = if self.expose_secrets {
                    export(&document, true)?
                } else if non_interactive || !stdin().is_terminal() || !stderr().is_terminal() {
                    export(&document, false)?
                } else {
                    let mut remaining_choice = None;
                    export_with_decisions(&document, |path, current, total| {
                        if let Some(choice) = remaining_choice {
                            return Ok(choice);
                        }
                        let selected = Select::new()
                            .with_prompt(format!(
                                "Secret {current}/{total}: Include {path} as a literal?"
                            ))
                            .items([
                                "No, mask with environment reference",
                                "Yes, include literal value",
                                "No to all, mask this and remaining secrets",
                                "Yes to all, include this and remaining secrets",
                            ])
                            .default(0)
                            .report(false)
                            .interact_opt()
                            .map_err(|_| ExportError::Prompt)?
                            .ok_or(ExportError::Prompt)?;
                        if selected >= 2 {
                            remaining_choice = Some(selected == 3);
                        }
                        Ok(selected == 1 || selected == 3)
                    })?
                };
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
