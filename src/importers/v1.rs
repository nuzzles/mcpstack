use crate::schema::{ValidationError, v1::StackV1};

pub(super) fn from_yaml(document: &str) -> Result<StackV1, ValidationError> {
    let stack: StackV1 =
        yaml_serde::from_str(document).map_err(|_| ValidationError::InvalidDocument)?;
    stack.validate()?;
    Ok(stack)
}
