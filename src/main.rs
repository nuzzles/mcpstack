mod cli;
mod cmd;
mod error;
mod exporters;
mod importers;
mod integrations;
mod io;
mod logging;
mod schema;
mod stack_source;
mod update;

use std::io::Write;
use std::process::ExitCode;

use clap::error::ErrorKind;
use cli::Cli;
use error::{AppError, ErrorCode};

async fn run() -> Result<ExitCode, AppError> {
    let cli = match Cli::parse_args() {
        Ok(cli) => cli,
        Err(error) => {
            if matches!(
                error.kind(),
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
            ) {
                error.print()?;
                return Ok(ExitCode::SUCCESS);
            }
            error.print()?;
            return Ok(ErrorCode::InvalidArgument.as_exit_code());
        }
    };

    cli.logging.init()?;
    cli.run(&mut std::io::stdout().lock()).await?;
    Ok(ExitCode::SUCCESS)
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    match run().await {
        Ok(code) => code,
        Err(error) => {
            let code = error.code();
            // Fatal diagnostics are command results, even when logs are filtered out.
            if tracing::enabled!(tracing::Level::ERROR) {
                tracing::error!("{}: {error}", code.as_ref());
            } else {
                let _ = writeln!(std::io::stderr().lock(), "{}: {error}", code.as_ref());
            }
            code.as_exit_code()
        }
    }
}
