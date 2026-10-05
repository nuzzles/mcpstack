use std::collections::BTreeMap;

use thiserror::Error;

use crate::schema::{SCHEMA_VERSION, StackV1};

/// Diagnostics deliberately omit parser contents, field values.
#[derive(Debug, Error)]
pub enum ExportError {
    #[error("Unable to parse Codex configuration as supported TOML data.")]
    Config,
    #[error("Codex mcp_servers and each server entry must be tables.")]
    Servers,
    #[error("Exported configuration cannot be represented by stack schema v1.")]
    Stack,
}

/// Export one native entry per server. This reads data only: it does not resolve
/// credentials, run helpers, or infer installed-client compatibility.
pub fn export(document: &str) -> Result<StackV1, ExportError> {
    let config: toml::Value = toml::from_str(document).map_err(|_| ExportError::Config)?;
    let servers = match config.get("mcp_servers") {
        None => serde_json::Map::new(),
        Some(toml::Value::Table(table)) => {
            // Datetimes and nonfinite floats have no lossless JSON equivalent.
            if table.values().any(contains_unsupported_value) {
                return Err(ExportError::Config);
            }
            serde_json::to_value(table)
                .map_err(|_| ExportError::Config)?
                .as_object()
                .cloned()
                .ok_or(ExportError::Servers)?
        }
        Some(_) => return Err(ExportError::Servers),
    };
    if servers.values().any(|server| !server.is_object()) {
        return Err(ExportError::Servers);
    }
    let definitions: BTreeMap<_, _> = servers
        .into_iter()
        .map(|(name, config)| (name, serde_json::json!({"client":"codex", "config":config})))
        .collect();
    let document = serde_json::json!({"schema_version":SCHEMA_VERSION,"servers":definitions});
    let stack: StackV1 = serde_json::from_value(document).map_err(|_| ExportError::Stack)?;
    stack.validate().map_err(|_| ExportError::Stack)?;
    Ok(stack)
}

fn contains_unsupported_value(value: &toml::Value) -> bool {
    match value {
        toml::Value::Datetime(_) => true,
        toml::Value::Float(value) => !value.is_finite(),
        toml::Value::Array(values) => values.iter().any(contains_unsupported_value),
        toml::Value::Table(values) => values.values().any(contains_unsupported_value),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: &str = r#"
model = "unrelated"
[mcp_servers.local]
command = "example"
args = ["--token", "fixture-secret"]
startup_timeout_ms = 0
[mcp_servers.local.oauth]
client_secret = "another-fixture-secret"
"#;

    #[test]
    fn preserves_native_values_without_transforming_them() {
        let value = serde_json::to_value(export(CONFIG).unwrap()).unwrap();
        assert_eq!(value["servers"]["local"]["config"]["startup_timeout_ms"], 0);
        assert_eq!(
            value["servers"]["local"]["config"]["args"][1],
            "fixture-secret"
        );
        assert_eq!(
            value["servers"]["local"]["config"]["oauth"]["client_secret"],
            "another-fixture-secret"
        );
        assert!(!value.to_string().contains("unrelated"));
    }

    #[test]
    fn rejects_invalid_tables_duplicates_and_unsupported_values() {
        for config in [
            "mcp_servers = 1",
            "[mcp_servers]\nx=1",
            "[mcp_servers.x]\ncommand='a'\ncommand='b'",
            "[mcp_servers.x]\ntime=1979-05-27T07:32:00Z",
            "[mcp_servers.x]\nvalue=nan",
        ] {
            assert!(export(config).is_err());
        }
        let stack = export("model='unrelated'").unwrap();
        assert!(stack.servers.is_empty());
    }
}
