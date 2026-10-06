pub mod v1;

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub use v1::{Server, StackV1};

/// A stack file decoded into its version-specific representation.
/// Untagged serialization keeps the existing schema_version-based wire format.
#[derive(Serialize)]
#[serde(untagged)]
pub enum Stack {
    V1(StackV1),
}

impl Stack {
    pub fn from_yaml(document: &str) -> Result<Self, ValidationError> {
        crate::importers::from_yaml(document)
    }
}

impl From<StackV1> for Stack {
    fn from(stack: StackV1) -> Self {
        Self::V1(stack)
    }
}

/// Stack format version currently supported for reading and writing.
pub const SCHEMA_VERSION: u64 = 1;

/// Validation diagnostics contain no document values, including credentials.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ValidationError {
    #[error("Malformed stack or unsupported fields. Check the schema format.")]
    InvalidDocument,
    #[error("Unsupported stack schema version; only version 1 is supported.")]
    UnsupportedVersion,
    #[error("Server names must be nonempty, trimmed, and contain no control characters.")]
    InvalidServerName,
    #[error(
        "STDIO commands and working directories must be nonempty and contain no control characters."
    )]
    InvalidProcess,
    #[error("Literal arguments and environment values must not contain NUL bytes.")]
    InvalidValue,
    #[error(
        "Environment variable names must match [A-Za-z_][A-Za-z0-9_]*; forwarded names must be unique."
    )]
    InvalidEnvironment,
    #[error("Secret references must name an environment variable matching [A-Za-z_][A-Za-z0-9_]*.")]
    InvalidSecretReference,
    #[error(
        "Remote endpoints must use the transport's URL scheme without embedded credentials or fragments."
    )]
    InvalidEndpoint,
    #[error(
        "HTTP header names must be valid tokens and literal values must contain no control characters."
    )]
    InvalidHeader,
    #[error("Timeouts must be positive, finite numbers of seconds.")]
    InvalidTimeout,
    #[error(
        "Tool names must be nonempty, trimmed, contain no control characters, and be unique within each list."
    )]
    InvalidTools,
}

/// Minimal header decoded before selecting a version-specific stack reader.
#[derive(Deserialize)]
pub struct StackVersion {
    pub schema_version: u64,
}

#[cfg(test)]
impl StackV1 {
    pub fn from_yaml(document: &str) -> Result<Self, ValidationError> {
        let Stack::V1(stack) = Stack::from_yaml(document)?;
        Ok(stack)
    }
}

#[cfg(test)]
mod tests;
