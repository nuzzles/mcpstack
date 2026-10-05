use std::io::IsTerminal;

use clap::{Args, ValueEnum};
use tracing_subscriber::EnvFilter;

use crate::error::AppError;

/// Logging controls apply to every command; operation results stay on stdout.
#[derive(Args)]
pub struct Logging {
    /// Increase logging verbosity (-v for debug, -vv for trace).
    #[arg(short, long, global = true, action = clap::ArgAction::Count)]
    verbose: u8,
    /// Limit application logs to warnings and errors.
    #[arg(short, long, global = true, conflicts_with = "verbose")]
    quiet: bool,
    /// Override the tracing filter (also read from RUST_LOG).
    #[arg(long, global = true, env = "RUST_LOG", conflicts_with_all = ["verbose", "quiet"])]
    log: Option<String>,
    /// ANSI color mode for logs on stderr.
    #[arg(long, global = true, env = "MCPSTACK_COLOR", value_enum, default_value_t = ColorMode::Auto)]
    color: ColorMode,
    /// Disable ANSI colors in logs; also honors nonempty NO_COLOR.
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
    pub fn init(&self) -> Result<(), AppError> {
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
        let no_color =
            self.no_color || std::env::var_os("NO_COLOR").is_some_and(|value| !value.is_empty());
        let colored = ansi_enabled(self.color, no_color, std::io::stderr().is_terminal());
        #[cfg(windows)]
        if colored {
            // Explicit always mode must still emit ANSI when redirected or when
            // no Windows console is attached. Console setup is best-effort.
            let _ = nu_ansi_term::enable_ansi_support();
        }
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_writer(std::io::stderr)
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
