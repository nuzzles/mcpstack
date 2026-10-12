mod cli;
mod cmd;
mod error;
mod exporters;
mod importers;
mod integrations;
mod io;
mod logging;
mod schema;

use std::io::{IsTerminal, Write};
use std::process::ExitCode;

use clap::{ColorChoice, CommandFactory, Parser, error::ErrorKind};
use cli::Cli;
use error::{AppError, ErrorCode};

async fn run() -> Result<ExitCode, AppError> {
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
            // Most Clap errors include the supplied value, which may be a credential.
            // A missing required argument only names the required field and command.
            if error.kind() == ErrorKind::MissingRequiredArgument {
                let args: Vec<_> = std::env::args_os().collect();
                let no_color = args.iter().any(|arg| arg == "--no-color")
                    || std::env::var_os("NO_COLOR").is_some_and(|value| !value.is_empty());
                let mode = args
                    .windows(2)
                    .find_map(|pair| {
                        (pair[0] == "--color").then(|| pair[1].to_string_lossy().into_owned())
                    })
                    .or_else(|| {
                        args.iter().find_map(|arg| {
                            arg.to_str()
                                .and_then(|value| value.strip_prefix("--color="))
                                .map(str::to_owned)
                        })
                    })
                    .or_else(|| std::env::var("MCPSTACK_COLOR").ok());
                let color = if no_color || mode.as_deref() == Some("never") {
                    ColorChoice::Never
                } else if mode.as_deref() == Some("always") {
                    ColorChoice::Always
                } else if std::io::stderr().is_terminal() {
                    ColorChoice::Auto
                } else {
                    ColorChoice::Never
                };
                error.with_cmd(&Cli::command().color(color)).print()?;
                writeln!(std::io::stderr().lock(), "Error code: INVALID_ARGUMENT")?;
                return Ok(ErrorCode::InvalidArgument.as_exit_code());
            }
            let code = ErrorCode::InvalidArgument;
            writeln!(std::io::stderr().lock(), "{}: {code}", code.as_ref())?;
            return Ok(code.as_exit_code());
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
            if tracing::dispatcher::has_been_set() {
                tracing::error!("{}: {error}", code.as_ref());
            } else {
                let _ = writeln!(std::io::stderr().lock(), "{}: {error}", code.as_ref());
            }
            code.as_exit_code()
        }
    }
}
