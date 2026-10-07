//! Validation against the current Codex MCP config schema, independent of CLI versions.
use std::sync::LazyLock;

use serde_json::Value;

static SCHEMA: LazyLock<jsonschema::Validator> = LazyLock::new(|| {
    let schema: Value = serde_json::from_str(include_str!("mcp.schema.json"))
        .expect("bundled Codex schema must be valid JSON");
    jsonschema::validator_for(&schema).expect("bundled Codex schema must compile")
});

/// Return only validity: validator diagnostics can contain credential values.
pub fn supports(definition: &Value) -> bool {
    SCHEMA.is_valid(definition)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn accepts_current_fields_and_rejects_obsolete_or_unknown_fields() {
        assert!(supports(&json!({
            "url": "https://example.com/mcp",
            "auth": "oauth",
            "oauth": {"client_id": "public-client", "callback_port": 1234},
            "tools": {"read": {"approval_mode": "prompt", "output_token_limit": 2000}},
            "bearer_token_env_var": "TOKEN"
        })));
        for definition in [
            json!({"url": "https://example.com", "bearer_token": "fixture-secret"}),
            json!({"command": "tool", "future": true}),
            json!({"command": 123}),
            json!({"url": "https://example.com", "oauth": {"callback_port": "invalid"}}),
        ] {
            assert!(!supports(&definition));
        }
    }
}
