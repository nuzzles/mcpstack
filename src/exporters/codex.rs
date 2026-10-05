use std::collections::{BTreeMap, BTreeSet};

use thiserror::Error;

use crate::schema::{SCHEMA_VERSION, Server, StackV1, v1::ClientValue};

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
pub fn export(document: &str, expose_secrets: bool) -> Result<StackV1, ExportError> {
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
    let mut stack: StackV1 = serde_json::from_value(document).map_err(|_| ExportError::Stack)?;
    stack.validate().map_err(|_| ExportError::Stack)?;
    if !expose_secrets {
        let mut names = BTreeSet::new();
        for server in stack.servers.values() {
            if let Server::ClientSpecific { config, .. } = server {
                for value in config.values() {
                    collect_references(value, &mut names);
                }
            }
        }
        for (server_name, server) in &mut stack.servers {
            if let Server::ClientSpecific { config, .. } = server {
                for (key, value) in config {
                    if key == "env_http_headers" {
                        continue;
                    }
                    protect(
                        value,
                        &format!("{server_name}_{key}"),
                        secret_key(key),
                        key == "args",
                        &mut names,
                    );
                }
            }
        }
    }
    Ok(stack)
}

fn collect_references(value: &ClientValue, names: &mut BTreeSet<String>) {
    match value {
        ClientValue::Object(values) => {
            if let Some(ClientValue::String(name)) = values.get("$env") {
                names.insert(name.clone());
            } else {
                for value in values.values() {
                    collect_references(value, names);
                }
            }
        }
        ClientValue::Array(values) => {
            for value in values {
                collect_references(value, names);
            }
        }
        _ => {}
    }
}

fn secret_key(key: &str) -> bool {
    let mut normalized = String::new();
    for c in key.chars() {
        if c.is_ascii_uppercase()
            && normalized
                .chars()
                .last()
                .is_some_and(|previous| previous.is_ascii_lowercase())
            && key.chars().any(|character| character.is_ascii_lowercase())
        {
            normalized.push('_');
        }
        normalized.push(if c.is_ascii_alphanumeric() {
            c.to_ascii_lowercase()
        } else {
            '_'
        });
    }
    // Uppercase environment/header names are common; collapse their separators.
    let compact: String = key
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect();
    if ["envvar", "envvars", "filename", "filepath", "file", "path"]
        .iter()
        .any(|suffix| compact.ends_with(suffix))
    {
        return false;
    }
    matches!(
        compact.as_str(),
        "authorization"
            | "proxyauthorization"
            | "cookie"
            | "setcookie"
            | "password"
            | "passwd"
            | "secret"
            | "token"
            | "apikey"
            | "accesskey"
            | "privatekey"
    ) || [
        "token",
        "secret",
        "password",
        "passwd",
        "apikey",
        "accesskey",
        "privatekey",
    ]
    .iter()
    .any(|suffix| compact.ends_with(suffix))
        || normalized
            .split('_')
            .any(|part| matches!(part, "secret" | "password" | "passwd"))
}

fn protect(
    value: &mut ClientValue,
    path: &str,
    sensitive: bool,
    arguments: bool,
    names: &mut BTreeSet<String>,
) {
    match value {
        ClientValue::Object(values) => {
            if !values.contains_key("$env") {
                for (key, value) in values {
                    protect(
                        value,
                        &format!("{path}_{key}"),
                        sensitive || secret_key(key),
                        key == "args",
                        names,
                    );
                }
            }
        }
        ClientValue::Array(values) => {
            let mut next_secret = false;
            for (index, value) in values.iter_mut().enumerate() {
                let (inline_secret, following_secret) = if arguments {
                    match value {
                        ClientValue::String(text) if text.starts_with('-') => {
                            let (flag, inline) = text
                                .split_once('=')
                                .map_or((text.as_str(), false), |(flag, _)| (flag, true));
                            let credential = secret_key(flag.trim_start_matches('-'));
                            (credential && inline, credential && !inline)
                        }
                        _ => (false, false),
                    }
                } else {
                    (false, false)
                };
                protect(
                    value,
                    &format!("{path}_{index}"),
                    sensitive || next_secret || inline_secret,
                    false,
                    names,
                );
                next_secret = following_secret;
            }
        }
        ClientValue::String(_) | ClientValue::Number(_) if sensitive => {
            // Never derive names from values or read the process environment.
            let suffix: String = path
                .chars()
                .map(|c| {
                    if c.is_ascii_alphanumeric() {
                        c.to_ascii_uppercase()
                    } else {
                        '_'
                    }
                })
                .collect();
            let base = format!("MCPSTACK_{suffix}");
            let mut name = base.clone();
            let mut index = 2;
            while !names.insert(name.clone()) {
                name = format!("{base}_{index}");
                index += 1;
            }
            *value = ClientValue::Object(BTreeMap::from([(
                "$env".to_owned(),
                ClientValue::String(name),
            )]));
        }
        _ => {}
    }
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
    use crate::exporters::to_yaml;

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
        let value = serde_json::to_value(export(CONFIG, true).unwrap()).unwrap();
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
    fn default_export_masks_credentials_and_round_trips() {
        let stack = export(CONFIG, false).unwrap();
        let value = serde_json::to_value(&stack).unwrap();
        assert_eq!(
            value["servers"]["local"]["config"]["args"][1],
            serde_json::json!({"$env":"MCPSTACK_LOCAL_ARGS_1"})
        );
        assert_eq!(
            value["servers"]["local"]["config"]["oauth"]["client_secret"],
            serde_json::json!({"$env":"MCPSTACK_LOCAL_OAUTH_CLIENT_SECRET"})
        );
        assert_eq!(value["servers"]["local"]["config"]["startup_timeout_ms"], 0);
        let yaml = to_yaml(&stack).unwrap();
        assert!(!yaml.contains("fixture-secret"));
        assert!(yaml.contains("example"));
        assert_eq!(yaml, to_yaml(&export(CONFIG, false).unwrap()).unwrap());
        let restored = StackV1::from_yaml(&yaml).unwrap();
        assert_eq!(serde_json::to_value(restored).unwrap(), value);
    }

    #[test]
    fn preserves_references_and_disambiguates_generated_names() {
        let config = r#"
[mcp_servers.x]
"api-key" = "first-secret"
"api_key" = "second-secret"
pin = 1234
enabled = true
[mcp_servers.x.existing]
"$env" = "MCPSTACK_X_API_KEY"
"#;
        let value = serde_json::to_value(export(config, false).unwrap()).unwrap();
        let config = &value["servers"]["x"]["config"];
        assert_eq!(config["existing"]["$env"], "MCPSTACK_X_API_KEY");
        assert_eq!(config["api-key"]["$env"], "MCPSTACK_X_API_KEY_2");
        assert_eq!(config["api_key"]["$env"], "MCPSTACK_X_API_KEY_3");
        assert_eq!(config["pin"], 1234);
        assert_eq!(config["enabled"], true);
        assert!(!value.to_string().contains("secret"));
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
            assert!(export(config, false).is_err());
        }
        let stack = export("model='unrelated'", false).unwrap();
        assert!(stack.servers.is_empty());
    }
}
