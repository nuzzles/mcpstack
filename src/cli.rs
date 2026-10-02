use clap::{Arg, ArgAction, Command};

pub const EXAMPLES: &[&str] = &[
    "mcpstack --schema",
    "mcpstack --json --non-interactive",
    "mcpstack --help",
];

pub fn command() -> Command {
    Command::new("mcpstack")
        .version(env!("CARGO_PKG_VERSION"))
        .about(env!("CARGO_PKG_DESCRIPTION"))
        .after_help(format!("Examples:\n  {}", EXAMPLES.join("\n  ")))
        .arg(
            Arg::new("schema")
                .long("schema")
                .action(ArgAction::SetTrue)
                .help("Print the complete CLI interface as versioned JSON"),
        )
        .arg(
            Arg::new("json")
                .long("json")
                .action(ArgAction::SetTrue)
                .help("Print results and errors as JSON (schema output is always JSON)"),
        )
        .arg(
            Arg::new("non-interactive")
                .long("non-interactive")
                .action(ArgAction::SetTrue)
                .help("Disable interactive prompts; fail when required inputs are missing"),
        )
}
