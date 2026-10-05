use crate::schema::{ValidationError, v1::StackV1};

pub(super) fn to_yaml(stack: &StackV1) -> Result<String, ValidationError> {
    stack.validate()?;
    yaml_serde::to_string(stack).map_err(|_| ValidationError::InvalidDocument)
}
