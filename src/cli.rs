mod diagnostics;
pub(crate) mod schema;

use crate::cmd::schema::Schema;
use crate::cmd::update::Update;
use crate::cmd::validate::Validate;
use crate::error::AppError;
use crate::integrations::Client;
use crate::logging::Logging;
use crate::update;
use clap::{CommandFactory, Parser, Subcommand};
use std::io::{IsTerminal, Write};

pub use crate::integrations::EXAMPLES;

/// Manage, share, and switch MCP server configurations across clients.
#[derive(Parser)]
#[command(
    name = "mcpstack",
    version,
    styles = diagnostics::styles(),
    about = env!("CARGO_PKG_DESCRIPTION"),
    after_help = format!("Examples:\n  {}", EXAMPLES.join("\n  "))
)]
pub struct Cli {
    #[command(flatten)]
    pub logging: Logging,
    /// Disable interactive prompts.
    #[arg(long, global = true)]
    non_interactive: bool,
    /// Print the complete CLI interface as versioned JSON.
    #[arg(long)]
    schema: bool,
    /// Skip automatic update checks (explicit update commands still work).
    #[arg(
        long,
        global = true,
        env = "MCPSTACK_SKIP_VERSION_CHECK",
        hide_env_values = true
    )]
    skip_version_check: bool,
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    Validate(Validate),
    Update(Update),
    #[command(flatten)]
    Client(Client),
}

impl Cli {
    fn should_check_updates(&self, interactive: bool) -> bool {
        !self.skip_version_check
            && !self.non_interactive
            && !self.logging.quiet()
            && interactive
            && matches!(&self.command, Some(Commands::Client(client)) if client.allows_update_check())
    }

    pub async fn run(self, output: &mut impl Write) -> Result<(), AppError> {
        let check = self.should_check_updates(
            std::io::stdin().is_terminal()
                && std::io::stdout().is_terminal()
                && std::io::stderr().is_terminal(),
        );
        if self.schema && self.command.is_some() {
            return Err(AppError::Arguments);
        }
        if self.schema {
            Schema.run(output)?;
            Ok(())
        } else if let Some(command) = self.command {
            match command {
                Commands::Validate(inner) => inner.run(output).await,
                Commands::Update(inner) => inner.run(output, self.non_interactive).await,
                Commands::Client(client) => {
                    client
                        .run(output, self.non_interactive, self.logging.output_ansi())
                        .await?;
                    if check {
                        update::notify().await;
                    }
                    Ok(())
                }
            }
        } else {
            Self::command().write_long_help(output)?;
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::FromArgMatches;

    fn parse(args: impl IntoIterator<Item = &'static str>) -> Cli {
        let matches = Cli::command()
            .mut_arg("log", |arg| arg.env(None::<&str>))
            .mut_arg("skip_version_check", |arg| arg.env(None::<&str>))
            .try_get_matches_from(args)
            .unwrap();
        Cli::from_arg_matches(&matches).unwrap()
    }

    #[test]
    fn update_checks_respect_read_only_operations_terminals_and_opt_out() {
        for harness in ["codex", "claude", "claude-desktop"] {
            for operation in ["export", "import", "use"] {
                let args = ["mcpstack", harness, operation, "stack.yaml"];
                let cli = parse(args);
                assert!(cli.should_check_updates(true));
                assert!(!cli.should_check_updates(false));
                for opt_out in ["--non-interactive", "--quiet", "--skip-version-check"] {
                    let cli = parse(args.into_iter().chain([opt_out]));
                    assert!(!cli.should_check_updates(true));
                }
            }
            for args in [
                vec!["mcpstack", harness, "list"],
                vec!["mcpstack", harness, "show"],
                vec!["mcpstack", harness, "diff", "stack.yaml"],
                vec!["mcpstack", harness, "import", "stack.yaml", "--dry-run"],
                vec!["mcpstack", harness, "use", "stack.yaml", "--dry-run"],
            ] {
                assert!(!parse(args).should_check_updates(true));
            }
        }
        for args in [
            vec!["mcpstack"],
            vec!["mcpstack", "--schema"],
            vec!["mcpstack", "validate", "stack.yaml"],
            vec!["mcpstack", "update", "--check"],
        ] {
            assert!(!parse(args).should_check_updates(true));
        }
    }
}
