//! Stack readers selected by the document's schema version.
mod v1;

use crate::schema::{StackV1, StackVersion, ValidationError};

pub fn from_yaml(document: &str) -> Result<StackV1, ValidationError> {
    let header: StackVersion =
        yaml_serde::from_str(document).map_err(|_| ValidationError::InvalidDocument)?;
    match header.schema_version {
        1 => v1::from_yaml(document),
        _ => Err(ValidationError::UnsupportedVersion),
    }
}
