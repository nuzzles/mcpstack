mod cli;
mod schema;

use std::io::{self, Write};
use std::process::ExitCode;

use clap::error::ErrorKind;
use serde::Serialize;

#[derive(Serialize)]
#[serde(untagged)]
enum Response<T> {
    Success { ok: bool, result: T },
    Failure { ok: bool, error: ErrorBody },
}

#[derive(Serialize)]
struct ErrorBody {
    code: &'static str,
    message: &'static str,
}

fn write_json(value: &impl Serialize) -> io::Result<()> {
    let mut stdout = io::stdout().lock();
    serde_json::to_writer_pretty(&mut stdout, value)?;
    writeln!(stdout)
}

fn run() -> io::Result<u8> {
    let args: Vec<_> = std::env::args_os().collect();
    // Parsing can fail before matches exist. Do not interpret anything after `--`.
    let json = args
        .iter()
        .skip(1)
        .take_while(|arg| *arg != "--")
        .any(|arg| arg == "--json" || arg == "--schema");
    let mut command = cli::command();
    let matches = match command.try_get_matches_from_mut(args) {
        Ok(matches) => matches,
        Err(error) => {
            match error.kind() {
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion => {
                    if json {
                        write_json(&Response::Success {
                            ok: true,
                            result: serde_json::json!({
                                "kind": if error.kind() == ErrorKind::DisplayHelp { "help" } else { "version" },
                                "text": error.to_string(),
                            }),
                        })?;
                    } else {
                        error.print()?;
                    }
                    return Ok(0);
                }
                _ => {
                    if json {
                        write_json(&Response::<()>::Failure {
                            ok: false,
                            error: ErrorBody {
                                code: "INVALID_ARGUMENT",
                                // Never echo untrusted argument values in machine-readable errors.
                                message: "Invalid CLI arguments. Run mcpstack --schema or --help for usage.",
                            },
                        })?;
                    } else {
                        error.print()?;
                    }
                    return Ok(2);
                }
            }
        }
    };

    if matches.get_flag("schema") {
        write_json(&schema::describe(command))?;
    } else if matches.get_flag("json") {
        write_json(&Response::Success {
            ok: true,
            result: serde_json::json!({
                "name": "mcpstack",
                "cli_version": env!("CARGO_PKG_VERSION"),
                "status": "in_development",
            }),
        })?;
    } else {
        writeln!(
            io::stdout().lock(),
            "mcpstack {} — work in progress",
            env!("CARGO_PKG_VERSION")
        )?;
    }
    Ok(0)
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => ExitCode::from(code),
        Err(_) => {
            let _ = writeln!(
                io::stderr().lock(),
                "OUTPUT_ERROR: Unable to write CLI output."
            );
            ExitCode::from(1)
        }
    }
}
