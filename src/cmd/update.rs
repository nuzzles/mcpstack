use crate::error::AppError;
use crate::update::{self, Error, Installation};
use clap::Args;
use dialoguer::Confirm;
use std::io::{IsTerminal, Write};

/// Check for or install the latest stable mcpstack release from crates.io.
#[derive(Args)]
pub struct Update {
    /// Check the registry without installing or writing an update cache.
    #[arg(long, conflicts_with = "auto_approve")]
    check: bool,
    /// Approve installing the update without prompting (requires Cargo).
    #[arg(short = 'y', long)]
    auto_approve: bool,
}

impl Update {
    pub async fn run(self, output: &mut impl Write, non_interactive: bool) -> Result<(), AppError> {
        if !self.check
            && !self.auto_approve
            && (non_interactive
                || !std::io::stdin().is_terminal()
                || !std::io::stderr().is_terminal())
        {
            return Err(Error::ApprovalRequired.into());
        }
        // Refuse development builds and package-managed binaries before networking.
        let installation = if self.check {
            None
        } else {
            Some(Installation::current()?)
        };
        let current = update::current_version();
        writeln!(output, "Installed version: {current}")?;
        let Some(latest) = update::latest().await? else {
            writeln!(
                output,
                "No stable mcpstack release is available on crates.io. No update was installed."
            )?;
            return Ok(());
        };
        writeln!(output, "Latest stable version: {latest}")?;
        if !update::is_newer(&latest, &current) {
            writeln!(output, "mcpstack is up to date. No update was installed.")?;
            return Ok(());
        }
        if let Some(installation) = installation {
            writeln!(
                output,
                "Install root: {}",
                installation.root().display().to_string().escape_debug()
            )?;
            output.flush()?;
            if !self.auto_approve
                && !Confirm::new()
                    .with_prompt(format!(
                        "Update mcpstack {current} to {latest} from crates.io?"
                    ))
                    .default(false)
                    .interact()
                    .map_err(|_| Error::ApprovalCancelled)?
            {
                writeln!(output, "Update declined. mcpstack was not updated.")?;
                return Ok(());
            }
            installation.install(&latest)?;
            writeln!(output, "Updated mcpstack: {current} -> {latest}.")?;
        } else {
            writeln!(
                output,
                "Update available. Run mcpstack update to install it."
            )?;
        }
        Ok(())
    }
}
