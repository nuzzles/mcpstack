//! Version-specific stack serialization.
mod v1;

use crate::schema::{Stack, ValidationError};

pub fn to_yaml(stack: &Stack) -> Result<String, ValidationError> {
    match stack {
        Stack::V1(stack) => v1::to_yaml(stack),
    }
}
