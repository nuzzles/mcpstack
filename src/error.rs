use crate::schema::ValidationError;
use std::io;
use std::process::ExitCode;

use serde::{Serialize, Serializer};
use strum::{AsRefStr, EnumIter};
use thiserror::Error;

/// Program failures and their unique process exit statuses.
#[derive(Clone, Copy, Debug, AsRefStr, EnumIter, Error)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[repr(u8)]
pub enum ErrorCode {
    #[error("Unable to write CLI output.")]
    OutputError = 1,
    #[error("Invalid CLI arguments. Run mcpstack --help for usage.")]
    InvalidArgument = 2,
    #[error("Unable to read the stack file. Check its path, permissions, and UTF-8 encoding.")]
    StackReadError = 3,
    #[error("Invalid stack. Check the schema format.")]
    InvalidStack = 4,
    #[error("Unable to read the client configuration. Check its path and permissions.")]
    ConfigReadError = 5,
    #[error("Unable to export client configuration.")]
    ExportError = 6,
    #[error("Unable to initialize logging. Check the log filter.")]
    LoggingError = 9,
}

impl ErrorCode {
    pub fn as_exit_code(self) -> ExitCode {
        ExitCode::from(self.as_u8())
    }

    pub const fn as_u8(self) -> u8 {
        self as u8
    }
}

impl Serialize for ErrorCode {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_ref())
    }
}

/// Execution errors preserve typed causes but expose only safe diagnostics.
#[derive(Debug, Error)]
pub enum AppError {
    #[error("{code}", code = ErrorCode::InvalidArgument)]
    Arguments,
    #[error("{code}", code = ErrorCode::LoggingError)]
    Logging,
    #[error("{code}", code = ErrorCode::OutputError)]
    Output(#[from] io::Error),
    #[error("{code}", code = ErrorCode::StackReadError)]
    StackRead(#[source] io::Error),
    #[error("{0}")]
    Stack(#[from] ValidationError),
    #[error("{code}", code = ErrorCode::ConfigReadError)]
    ConfigRead(#[source] io::Error),
    #[error("Cannot determine the Codex config path. Set CODEX_HOME.")]
    ConfigPath,
    #[error("{0}")]
    Export(#[from] crate::exporters::codex::ExportError),
}

impl AppError {
    pub fn code(&self) -> ErrorCode {
        match self {
            Self::Logging => ErrorCode::LoggingError,
            Self::Arguments => ErrorCode::InvalidArgument,
            Self::Output(_) => ErrorCode::OutputError,
            Self::StackRead(_) => ErrorCode::StackReadError,
            Self::Stack(_) => ErrorCode::InvalidStack,
            Self::ConfigRead(_) | Self::ConfigPath => ErrorCode::ConfigReadError,
            Self::Export(_) => ErrorCode::ExportError,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use strum::IntoEnumIterator;

    #[test]
    fn external_codes_and_failure_statuses_are_unique() {
        let mut names = HashSet::new();
        let mut statuses = HashSet::new();
        for code in ErrorCode::iter() {
            assert!(
                names.insert(code.as_ref().to_owned()),
                "Duplicate error code: {code}"
            );
            assert!(statuses.insert(code.as_u8()), "Duplicate exit status");
            assert_ne!(code.as_u8(), 0, "Failures must not report success");
        }
    }
}
