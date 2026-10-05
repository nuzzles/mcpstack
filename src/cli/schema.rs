use crate::error::ErrorCode;
use clap::{Arg, ArgAction, Command};
use serde::Serialize;
use serde_json::Value;
use strum::IntoEnumIterator;

#[derive(Serialize)]
pub struct CliSchema {
    schema_version: u32,
    cli_version: &'static str,
    command: CommandSchema,
    examples: &'static [&'static str],
    exit_statuses: Vec<ExitStatus>,
}

#[derive(Serialize)]
struct ExitStatus {
    status: u8,
    description: String,
    error_code: Option<ErrorCode>,
}

#[derive(Serialize)]
struct CommandSchema {
    name: String,
    description: String,
    subcommand_required: bool,
    args_conflicts_with_subcommands: bool,
    arguments: Vec<ArgumentSchema>,
    options: Vec<ArgumentSchema>,
    commands: Vec<CommandSchema>,
}

#[derive(Serialize)]
struct ArgumentSchema {
    name: String,
    long: Option<String>,
    short: Option<char>,
    long_aliases: Vec<String>,
    short_aliases: Vec<char>,
    description: String,
    action: &'static str,
    value_type: &'static str,
    required: bool,
    default: Vec<Value>,
    possible_values: Vec<String>,
    min_values: usize,
    max_values: Option<usize>,
    conflicts_with: Vec<String>,
}

// FIXME: Replace the custom CLI description with upstream JSON export when available.
// https://github.com/clap-rs/clap/issues/918
// https://github.com/clap-rs/clap/issues/6299
pub fn describe(mut command: Command) -> CliSchema {
    command.build();
    CliSchema {
        schema_version: 1,
        cli_version: env!("CARGO_PKG_VERSION"),
        command: describe_command(&command),
        examples: crate::cli::EXAMPLES,
        exit_statuses: std::iter::once(ExitStatus {
            status: 0,
            description: "Success, help, or version".to_owned(),
            error_code: None,
        })
        .chain(ErrorCode::iter().map(|code| ExitStatus {
            status: code.as_u8(),
            description: code.to_string(),
            error_code: Some(code),
        }))
        .collect(),
    }
}

// FIXME: Delegate command-tree serialization to Clap when JSON/Serde support lands.
// https://github.com/clap-rs/clap/issues/918
// https://github.com/clap-rs/clap/issues/6299
fn describe_command(command: &Command) -> CommandSchema {
    let mut arguments: Vec<_> = command.get_arguments().collect();
    arguments.sort_by_key(|arg| (arg.get_index(), arg.get_id().as_str()));
    let mut commands: Vec<_> = command.get_subcommands().collect();
    commands.sort_by_key(|command| command.get_name());
    CommandSchema {
        name: command.get_name().to_owned(),
        description: command
            .get_about()
            .map(ToString::to_string)
            .unwrap_or_default(),
        subcommand_required: command.is_subcommand_required_set(),
        args_conflicts_with_subcommands: command.is_args_conflicts_with_subcommands_set(),
        arguments: arguments
            .iter()
            .filter(|arg| arg.is_positional())
            .map(|arg| describe_arg(command, arg))
            .collect(),
        options: arguments
            .iter()
            .filter(|arg| !arg.is_positional())
            .map(|arg| describe_arg(command, arg))
            .collect(),
        commands: commands.into_iter().map(describe_command).collect(),
    }
}

// FIXME: Use upstream argument serialization to avoid maintaining a partial metadata mapping.
// https://github.com/clap-rs/clap/issues/918
// https://github.com/clap-rs/clap/issues/6299
fn describe_arg(command: &Command, arg: &Arg) -> ArgumentSchema {
    let possible_values: Vec<_> = arg
        .get_possible_values()
        .into_iter()
        .map(|value| value.get_name().to_owned())
        .collect();
    let value_type = if !arg.get_action().takes_values() {
        if matches!(arg.get_action(), ArgAction::Count) {
            "integer"
        } else {
            "boolean"
        }
    } else if !possible_values.is_empty() {
        "enum"
    } else {
        "string"
    };
    let range = arg.get_num_args().unwrap_or_default();
    let mut conflicts_with: Vec<_> = command
        .get_arg_conflicts_with(arg)
        .into_iter()
        .map(|arg| arg.get_id().to_string())
        .collect();
    conflicts_with.sort();
    ArgumentSchema {
        name: arg.get_id().to_string(),
        long: arg.get_long().map(|long| format!("--{long}")),
        short: arg.get_short(),
        long_aliases: arg
            .get_all_aliases()
            .unwrap_or_default()
            .into_iter()
            .map(|alias| format!("--{alias}"))
            .collect(),
        short_aliases: arg.get_all_short_aliases().unwrap_or_default(),
        description: arg
            .get_long_help()
            .or_else(|| arg.get_help())
            .map(ToString::to_string)
            .unwrap_or_default(),
        action: match arg.get_action() {
            ArgAction::Set => "set",
            ArgAction::Append => "append",
            ArgAction::SetTrue => "set_true",
            ArgAction::SetFalse => "set_false",
            ArgAction::Count => "count",
            ArgAction::Help => "help",
            ArgAction::HelpShort => "help_short",
            ArgAction::HelpLong => "help_long",
            ArgAction::Version => "version",
            _ => "unknown",
        },
        value_type,
        required: arg.is_required_set(),
        default: arg
            .get_default_values()
            .iter()
            .map(|value| {
                let value = value.to_string_lossy();
                match value_type {
                    "boolean" | "integer" => serde_json::from_str(&value)
                        .unwrap_or_else(|_| Value::String(value.into_owned())),
                    _ => Value::String(value.into_owned()),
                }
            })
            .collect(),
        possible_values,
        min_values: range.min_values(),
        max_values: (range.max_values() != usize::MAX).then_some(range.max_values()),
        conflicts_with,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_metadata_survives_schema_export() {
        let command = Command::new("fixture").subcommand(
            Command::new("import")
                .arg(Arg::new("file").required(true))
                .arg(
                    Arg::new("mode")
                        .long("mode")
                        .alias("policy")
                        .value_parser(["merge", "replace"])
                        .default_value("merge"),
                )
                .arg(
                    Arg::new("dry-run")
                        .long("dry-run")
                        .action(ArgAction::SetTrue)
                        .conflicts_with("mode"),
                ),
        );
        let schema = serde_json::to_value(describe(command.clone())).unwrap();
        let import = schema["command"]["commands"]
            .as_array()
            .unwrap()
            .iter()
            .find(|command| command["name"] == "import")
            .unwrap();
        assert_eq!(import["arguments"][0]["name"], "file");
        assert_eq!(import["arguments"][0]["required"], true);
        let options = import["options"].as_array().unwrap();
        let mode = options.iter().find(|arg| arg["name"] == "mode").unwrap();
        assert_eq!(mode["value_type"], "enum");
        assert_eq!(
            mode["possible_values"],
            serde_json::json!(["merge", "replace"])
        );
        assert_eq!(mode["default"], serde_json::json!(["merge"]));
        assert_eq!(mode["long_aliases"], serde_json::json!(["--policy"]));
        let dry_run = options.iter().find(|arg| arg["name"] == "dry-run").unwrap();
        assert_eq!(dry_run["conflicts_with"], serde_json::json!(["mode"]));
        assert!(
            command
                .clone()
                .try_get_matches_from(["fixture", "import", "stack.toml", "--policy", "merge"])
                .is_ok()
        );
        assert!(
            command
                .clone()
                .try_get_matches_from(["fixture", "import", "stack.toml", "--mode", "invalid"])
                .is_err()
        );
        assert!(
            command
                .clone()
                .try_get_matches_from(["fixture", "import"])
                .is_err()
        );
        assert!(
            command
                .try_get_matches_from([
                    "fixture",
                    "import",
                    "stack.toml",
                    "--mode",
                    "merge",
                    "--dry-run"
                ])
                .is_err()
        );
    }
}
