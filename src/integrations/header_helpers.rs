//! Configuration-only checks: helper commands are never executed.
use serde_json::Value;

use crate::schema::{ValidationError, v1::HeaderHelper};

pub(super) fn validate_native(definition: &Value) -> Result<(), ValidationError> {
    let Some(fields) = definition.as_object() else {
        return Err(ValidationError::InvalidDocument);
    };
    let helpers: Vec<_> = ["headersHelper", "http_headers_helper"]
        .iter()
        .filter_map(|key| fields.get(*key))
        .collect();
    if helpers.is_empty() {
        return Ok(());
    }
    if helpers.len() != 1
        || !fields.get("url").is_some_and(Value::is_string)
        || fields.contains_key("command")
        || fields.contains_key("environment_id")
    {
        return Err(ValidationError::InvalidHeaderHelper);
    }
    let command = helpers[0]
        .as_str()
        .ok_or(ValidationError::InvalidHeaderHelper)?;
    HeaderHelper::Literal(command.into()).validate()?;
    Ok(())
}
