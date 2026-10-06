use std::io::Write;
use std::path::PathBuf;
use std::{env, fs};

use clap::{Args, Subcommand};

use crate::error::AppError;
use crate::importers::codex::prepare;
use crate::importers::codex_fs::Snapshot;
use crate::schema::StackV1;

/// Merge a YAML stack into client configuration without installing servers.
#[derive(Args)]
pub struct Import {
    #[command(subcommand)]
    client: Client,
}

#[derive(Subcommand)]
enum Client {
    Codex {
        /// Stack file to import; secret references resolve from the environment.
        #[arg(value_name = "FILE")]
        file: PathBuf,
        /// Write this Codex TOML file instead of the default configuration.
        #[arg(long, value_name = "PATH")]
        config: Option<PathBuf>,
    },
}

impl Import {
    pub fn run(self, output: &mut impl Write) -> Result<(), AppError> {
        match self.client {
            Client::Codex { file, config } => {
                let path = config
                    .or_else(super::export::default_config)
                    .ok_or(AppError::ConfigPath)?;
                // Backup must succeed before stack reads or parsing.
                let snapshot = Snapshot::backup(&path)?;
                let document = fs::read_to_string(file).map_err(AppError::StackRead)?;
                let stack = StackV1::from_yaml(&document)?;
                let definitions = prepare(&stack, |name| env::var(name).ok())?;
                let added = snapshot.apply(&definitions)?;
                writeln!(
                    output,
                    "Imported {added} server(s); existing identical entries were unchanged."
                )?;
                Ok(())
            }
        }
    }
}
