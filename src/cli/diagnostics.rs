use std::ffi::OsString;

use clap::builder::styling::{AnsiColor, Styles};
use clap::error::{ContextKind, ContextValue, ErrorKind};
use clap::{ColorChoice, Command, CommandFactory, FromArgMatches};

use super::Cli;

// Match the Cargo/rustup palette using Clap's existing styling support.
pub(super) fn styles() -> Styles {
    Styles::styled()
        .header(AnsiColor::BrightGreen.on_default().bold())
        .usage(AnsiColor::BrightGreen.on_default().bold())
        .literal(AnsiColor::BrightCyan.on_default().bold())
        .placeholder(AnsiColor::Cyan.on_default())
        .error(AnsiColor::BrightRed.on_default().bold())
        .valid(AnsiColor::BrightCyan.on_default().bold())
        .invalid(AnsiColor::Yellow.on_default().bold())
}

impl Cli {
    pub fn parse_args() -> Result<Self, clap::Error> {
        let args: Vec<_> = std::env::args_os().collect();
        // Keep usage stable across platforms and avoid echoing a custom argv[0].
        let mut command = Self::command().bin_name("mcpstack");
        let color = diagnostic_color(&command, &args);
        command = command.color(color);
        let matches = command
            .try_get_matches_from_mut(args)
            .map_err(|error| redact_values(error, &command))?;
        let cli = Self::from_arg_matches(&matches)?;
        if cli.schema && cli.command.is_some() {
            let subcommand = matches.subcommand_name().unwrap_or("a subcommand");
            return Err(conflict(&mut command, "--schema", subcommand));
        }
        if let Some((first, second)) = cli.logging.conflicting_arguments() {
            return Err(conflict(&mut command, first, second));
        }
        Ok(cli)
    }
}

fn conflict(command: &mut Command, first: &str, second: &str) -> clap::Error {
    let mut error = clap::Error::new(ErrorKind::ArgumentConflict).with_cmd(command);
    error.insert(ContextKind::InvalidArg, ContextValue::String(first.into()));
    error.insert(ContextKind::PriorArg, ContextValue::String(second.into()));
    error.insert(
        ContextKind::Usage,
        ContextValue::StyledStr(command.render_usage()),
    );
    error
}

// Color has to be selected before parsing, including when parsing fails. Consume
// known option values and stop at `--`, so a path named --no-color is not a flag.
fn diagnostic_color(command: &Command, args: &[OsString]) -> ColorChoice {
    let mut mode = std::env::var("MCPSTACK_COLOR").unwrap_or_default();
    let mut no_color = std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty());
    let mut args = args.iter().skip(1).peekable();
    while let Some(arg) = args.next() {
        if arg == "--" {
            break;
        }
        if arg == "--no-color" {
            no_color = true;
        } else if let Some(arg) = arg.to_str().and_then(|v| v.strip_prefix("--")) {
            let (name, inline) = arg
                .split_once('=')
                .map_or((arg, None), |(n, v)| (n, Some(v)));
            if takes_value(command, name) {
                let value = inline.or_else(|| {
                    // Current value-taking options do not accept hyphen-prefixed
                    // values unless attached with `=`. Leave those flags to scan.
                    if args.peek().is_some_and(|value| {
                        value
                            .to_str()
                            .is_some_and(|v| !v.starts_with('-') || v == "-")
                    }) {
                        args.next().and_then(|v| v.to_str())
                    } else {
                        None
                    }
                });
                if name == "color" {
                    mode = value.unwrap_or_default().to_owned();
                }
            }
        }
    }
    if no_color {
        ColorChoice::Never
    } else {
        match mode.as_str() {
            "always" => ColorChoice::Always,
            "never" => ColorChoice::Never,
            _ => ColorChoice::Auto,
        }
    }
}

fn takes_value(command: &Command, name: &str) -> bool {
    command
        .get_arguments()
        .any(|arg| arg.get_long() == Some(name) && arg.get_action().takes_values())
        || command
            .get_subcommands()
            .any(|command| takes_value(command, name))
}

fn redact_values(error: clap::Error, command: &Command) -> clap::Error {
    if matches!(
        error.kind(),
        ErrorKind::DisplayHelp
            | ErrorKind::DisplayVersion
            | ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
    ) {
        return error;
    }
    // Rebuild from structured context rather than a validator's opaque message
    // or source, which could repeat a credential. Keep Clap's usage and spelling
    // suggestions, but omit free-form suggestions that repeat supplied values.
    let mut safe = clap::Error::new(error.kind()).with_cmd(command);
    for (kind, value) in error.context() {
        let value = match (kind, value) {
            (ContextKind::Suggested | ContextKind::SuggestedCommand | ContextKind::Custom, _) => {
                continue;
            }
            (ContextKind::InvalidValue, ContextValue::String(value)) if !value.is_empty() => {
                ContextValue::String("<redacted>".into())
            }
            (ContextKind::InvalidArg, ContextValue::String(value))
                if error.kind() == ErrorKind::UnknownArgument =>
            {
                ContextValue::String(option_name(value))
            }
            (ContextKind::InvalidSubcommand, ContextValue::String(value))
                if error.kind() == ErrorKind::InvalidSubcommand =>
            {
                ContextValue::String(if is_name(value) {
                    value.clone()
                } else {
                    "<redacted>".into()
                })
            }
            _ => value.clone(),
        };
        safe.insert(kind, value);
    }
    safe
}

fn option_name(value: &str) -> String {
    let (name, assigned) = value
        .split_once('=')
        .map_or((value, false), |(name, _)| (name, true));
    if name.starts_with('-') && is_name(name) {
        format!("{name}{}", if assigned { "=<redacted>" } else { "" })
    } else {
        "<redacted>".into()
    }
}

fn is_name(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
}
