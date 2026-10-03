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
