use std::env::var_os;
use std::io::{IsTerminal, stderr, stdout};

use clap::{ArgAction, Args, ValueEnum};
#[cfg(windows)]
use nu_ansi_term::enable_ansi_support;
use tracing_subscriber::{EnvFilter, fmt};

use crate::error::AppError;

/// Logging controls apply to every command; operation results stay on stdout.
#[derive(Args)]
pub struct Logging {
    /// Increase logging verbosity (-v for debug, -vv for trace).
    #[arg(short, long, global = true, action = ArgAction::Count)]
    verbose: u8,
    /// Limit application logs to warnings and errors.
    #[arg(short, long, global = true, conflicts_with = "verbose")]
    quiet: bool,
    /// Override the tracing filter (also read from RUST_LOG).
    #[arg(long, global = true, env = "RUST_LOG", conflicts_with_all = ["verbose", "quiet"])]
    log: Option<String>,
    /// ANSI color mode for logs and import diffs.
    #[arg(long, global = true, env = "MCPSTACK_COLOR", value_enum, default_value_t = ColorMode::Auto)]
    color: ColorMode,
    /// Disable ANSI colors; also honors nonempty NO_COLOR.
    #[arg(long, global = true)]
    no_color: bool,
}

#[derive(Clone, Copy, Default, ValueEnum)]
enum ColorMode {
    #[default]
    Auto,
    Always,
    Never,
}

impl Logging {
    /// Diffs follow stdout's terminal state independently of stderr logging.
    pub fn output_ansi(&self) -> bool {
        self.ansi_for(stdout().is_terminal())
    }

    fn ansi_for(&self, terminal: bool) -> bool {
        let no_color = self.no_color || var_os("NO_COLOR").is_some_and(|value| !value.is_empty());
        let colored = ansi_enabled(self.color, no_color, terminal);
        #[cfg(windows)]
        if colored {
            // Explicit always mode emits ANSI even without an attached console.
            let _ = enable_ansi_support();
        }
        colored
    }

    pub fn init(&self) -> Result<(), AppError> {
        // Clap checks conflicts within each command level, but global options
        // can be supplied on opposite sides of a subcommand.
        if (self.verbose != 0 && self.quiet)
            || (self.log.is_some() && (self.verbose != 0 || self.quiet))
        {
            return Err(AppError::Arguments);
        }
        let directives = self
            .log
            .as_deref()
            .unwrap_or(match (self.verbose, self.quiet) {
                (_, true) => "error,mcpstack=warn",
                (0, false) => "error,mcpstack=info",
                (1, false) => "error,mcpstack=debug",
                _ => "error,mcpstack=trace",
            });
        let filter = EnvFilter::try_new(directives).map_err(|_| AppError::Logging)?;
        let colored = self.ansi_for(stderr().is_terminal());
        fmt()
            .with_env_filter(filter)
            .with_writer(stderr)
            .with_ansi(colored)
            .with_target(false)
            .compact()
            .try_init()
            .map_err(|_| AppError::Logging)
    }
}

fn ansi_enabled(mode: ColorMode, no_color: bool, terminal: bool) -> bool {
    !no_color
        && match mode {
            ColorMode::Auto => terminal,
            ColorMode::Always => true,
            ColorMode::Never => false,
        }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ansi_auto_uses_stderr_and_no_color_overrides_all_modes() {
        assert!(ansi_enabled(ColorMode::Auto, false, true));
        assert!(!ansi_enabled(ColorMode::Auto, false, false));
        assert!(ansi_enabled(ColorMode::Always, false, false));
        assert!(!ansi_enabled(ColorMode::Never, false, true));
        for mode in [ColorMode::Auto, ColorMode::Always, ColorMode::Never] {
            assert!(!ansi_enabled(mode, true, true));
        }
    }
}
