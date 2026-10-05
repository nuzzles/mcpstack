//! Client configuration readers and version-specific stack serialization.
pub mod codex;
mod v1;

use crate::schema::{StackV1, ValidationError};

pub fn to_yaml(stack: &StackV1) -> Result<String, ValidationError> {
    match stack.schema_version {
        1 => v1::to_yaml(stack),
        _ => Err(ValidationError::UnsupportedVersion),
    }
}
