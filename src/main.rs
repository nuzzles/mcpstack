mod cli;
mod cmd;
mod error;
mod exporters;
mod importers;
mod integrations;
mod logging;
mod schema;

use std::io::{self, Write};
use std::process::ExitCode;

use clap::{Parser, error::ErrorKind};
use cli::Cli;
use error::{AppError, ErrorCode};

fn run() -> Result<ExitCode, AppError> {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) => {
            if matches!(
                error.kind(),
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
            ) {
                error.print()?;
                return Ok(ExitCode::SUCCESS);
            }
            // Argument values may contain credentials; never echo them in diagnostics.
            let code = ErrorCode::InvalidArgument;
            writeln!(io::stderr().lock(), "{}: {code}", code.as_ref())?;
            return Ok(code.as_exit_code());
        }
    };

    cli.logging.init()?;
    cli.run(&mut io::stdout().lock())?;
    Ok(ExitCode::SUCCESS)
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(error) => {
            let code = error.code();
            if tracing::dispatcher::has_been_set() {
                tracing::error!("{}: {error}", code.as_ref());
            } else {
                let _ = writeln!(io::stderr().lock(), "{}: {error}", code.as_ref());
            }
            code.as_exit_code()
        }
    }
}
