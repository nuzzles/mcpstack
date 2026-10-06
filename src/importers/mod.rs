//! Stack readers selected by the document's schema version.
pub mod codex;
mod v1;

use crate::schema::{Stack, StackVersion, ValidationError};

pub fn from_yaml(document: &str) -> Result<Stack, ValidationError> {
    let header: StackVersion =
        yaml_serde::from_str(document).map_err(|_| ValidationError::InvalidDocument)?;
    match header.schema_version {
        1 => v1::from_yaml(document).map(Stack::V1),
        _ => Err(ValidationError::UnsupportedVersion),
    }
}
pub mod codex_fs;
