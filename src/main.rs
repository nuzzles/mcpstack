mod cli;
mod cmd;
mod error;

use std::io::{self, Write};
use std::process::ExitCode;

use clap::{Parser, error::ErrorKind};
use cli::Cli;
use error::ErrorCode;

fn run() -> io::Result<ExitCode> {
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

    cli.run(&mut io::stdout().lock())?;
    Ok(ExitCode::SUCCESS)
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(_) => {
            let code = ErrorCode::OutputError;
            let _ = writeln!(io::stderr().lock(), "{}: {code}", code.as_ref());
            code.as_exit_code()
        }
    }
}
