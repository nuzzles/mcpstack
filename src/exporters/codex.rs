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
    #[error("MCP server fields do not match the supported current Codex config schema.")]
    Schema,
    #[error("Exported configuration cannot be represented by stack schema v1.")]
    Stack,
    #[error("Secret selection was cancelled or could not be completed. No stack was exported.")]
    Prompt,
}

/// Export one native entry per server. This reads data only: it does not resolve
/// credentials, run helpers, or infer installed-client compatibility.
pub fn export(document: &str, expose_secrets: bool) -> Result<StackV1, ExportError> {
    export_with_decisions(document, |_, _, _| Ok(expose_secrets))
}

/// Decide whether to expose each detected credential. The callback receives
/// only a field path and its one-based position and total, never the value.
pub fn export_with_decisions(
    document: &str,
    mut expose: impl FnMut(&str, usize, usize) -> Result<bool, ExportError>,
) -> Result<StackV1, ExportError> {
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
    if !servers.values().all(crate::adapters::codex::supports) {
        return Err(ExportError::Schema);
    }
    let definitions: BTreeMap<_, _> = servers
        .into_iter()
        .map(|(name, config)| (name, serde_json::json!({"client":"codex", "config":config})))
        .collect();
    let document = serde_json::json!({"schema_version":SCHEMA_VERSION,"servers":definitions});
    let mut stack: StackV1 = serde_json::from_value(document).map_err(|_| ExportError::Stack)?;
    stack.validate().map_err(|_| ExportError::Stack)?;
    let mut names = BTreeSet::new();
    for server in stack.servers.values() {
        if let Server::ClientSpecific { config, .. } = server {
            for value in config.values() {
                collect_references(value, &mut names);
            }
        }
    }
    let mut total = 0;
    visit_credentials(&mut stack, &mut names, &mut |_| {
        total += 1;
        Ok(true)
    })?;
    let mut current = 0;
    visit_credentials(&mut stack, &mut names, &mut |path| {
        current += 1;
        expose(path, current, total)
    })?;
    Ok(stack)
}

fn visit_credentials(
    stack: &mut StackV1,
    names: &mut BTreeSet<String>,
    expose: &mut impl FnMut(&str) -> Result<bool, ExportError>,
) -> Result<(), ExportError> {
    for (server_name, server) in &mut stack.servers {
        if let Server::ClientSpecific { config, .. } = server {
            for (key, value) in config {
                if key == "env_http_headers" {
                    continue;
                }
                protect(
                    value,
                    &format!("{server_name}_{key}"),
                    &format!("{server_name}.{key}"),
                    secret_key(key),
                    key == "args",
                    names,
                    expose,
                )?;
            }
        }
    }
    Ok(())
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
    let characters: Vec<_> = key.chars().collect();
    for (index, &c) in characters.iter().enumerate() {
        if c.is_ascii_uppercase()
            && index > 0
            && (characters[index - 1].is_ascii_lowercase()
                || (characters[index - 1].is_ascii_uppercase()
                    && characters
                        .get(index + 1)
                        .is_some_and(char::is_ascii_lowercase)))
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
        || {
            let parts: Vec<_> = normalized
                .split('_')
                .filter(|part| !part.is_empty())
                .collect();
            (parts.contains(&"token")
                && !parts.iter().any(|part| {
                    matches!(
                        *part,
                        "limit" | "count" | "budget" | "usage" | "length" | "size"
                    )
                }))
                || parts.windows(2).any(|pair| {
                    matches!(
                        pair,
                        ["api", "key"] | ["access", "key"] | ["private", "key"]
                    )
                })
        }
}

fn protect(
    value: &mut ClientValue,
    path: &str,
    display_path: &str,
    sensitive: bool,
    arguments: bool,
    names: &mut BTreeSet<String>,
    expose: &mut impl FnMut(&str) -> Result<bool, ExportError>,
) -> Result<(), ExportError> {
    match value {
        ClientValue::Object(values) => {
            if !values.contains_key("$env") {
                for (key, value) in values {
                    protect(
                        value,
                        &format!("{path}_{key}"),
                        &format!("{display_path}.{key}"),
                        sensitive || secret_key(key),
                        key == "args",
                        names,
                        expose,
                    )?;
                }
            }
        }
        ClientValue::Array(values) => {
            let mut next_secret = false;
            let mut options = arguments;
            for (index, value) in values.iter_mut().enumerate() {
                if options
                    && !next_secret
                    && matches!(value, ClientValue::String(text) if text == "--")
                {
                    options = false;
                }
                let (inline_secret, following_secret) = if options && !next_secret {
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
                    &format!("{display_path}[{index}]"),
                    sensitive || next_secret || inline_secret,
                    false,
                    names,
                    expose,
                )?;
                next_secret = following_secret;
            }
        }
        ClientValue::String(_) | ClientValue::Number(_) if sensitive => {
            if expose(display_path)? {
                return Ok(());
            }
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
    Ok(())
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
        let yaml = to_yaml(&stack.into()).unwrap();
        assert!(!yaml.contains("fixture-secret"));
        assert!(yaml.contains("example"));
        assert_eq!(
            yaml,
            to_yaml(&export(CONFIG, false).unwrap().into()).unwrap()
        );
        let restored = StackV1::from_yaml(&yaml).unwrap();
        assert_eq!(serde_json::to_value(restored).unwrap(), value);
    }

    #[test]
    fn preserves_references_and_disambiguates_generated_names() {
        // Reference preservation is a stack/redaction invariant; reference
        // objects are not valid literal Codex config fields.
        let mut stack = StackV1::from_yaml("schema_version: 1\nservers:\n  x:\n    client: codex\n    config:\n      api-key: first-secret\n      api_key: second-secret\n      pin: 1234\n      enabled: true\n      existing: {'$env': MCPSTACK_X_API_KEY}\n").unwrap();
        let mut names = BTreeSet::new();
        if let Server::ClientSpecific { config, .. } = &stack.servers["x"] {
            for value in config.values() {
                collect_references(value, &mut names);
            }
        }
        visit_credentials(&mut stack, &mut names, &mut |_| Ok(false)).unwrap();
        let value = serde_json::to_value(stack).unwrap();
        let config = &value["servers"]["x"]["config"];
        assert_eq!(config["existing"]["$env"], "MCPSTACK_X_API_KEY");
        assert_eq!(config["api-key"]["$env"], "MCPSTACK_X_API_KEY_2");
        assert_eq!(config["api_key"]["$env"], "MCPSTACK_X_API_KEY_3");
        assert_eq!(config["pin"], 1234);
        assert_eq!(config["enabled"], true);
        assert!(!value.to_string().contains("secret"));
    }

    #[test]
    fn credential_qualifiers_are_masked_without_masking_token_settings() {
        let document = r#"
[mcp_servers.example]
command = "example"
[mcp_servers.example.tools.read]
output_token_limit = 512
[mcp_servers.example.env]
tokenCount = "12"
tokenBudget = "2048"
TOKEN_VALUE = "first-synthetic-secret"
tokenValue = "second-synthetic-secret"
GITHUB_TOKEN_RAW = "third-synthetic-secret"
API_KEY_VALUE = "fourth-synthetic-secret"
AWS_ACCESS_KEY_ID = "synthetic-key-id"
[mcp_servers.remote]
url = "https://example.com/mcp"
bearer_token_env_var = "SERVICE_TOKEN"
"#;
        let stack = export(document, false).unwrap();
        let value = serde_json::to_value(&stack).unwrap();
        let config = &value["servers"]["example"]["config"];
        for key in [
            "TOKEN_VALUE",
            "tokenValue",
            "GITHUB_TOKEN_RAW",
            "API_KEY_VALUE",
            "AWS_ACCESS_KEY_ID",
        ] {
            assert!(config["env"][key]["$env"].is_string(), "{key}");
        }
        assert_eq!(config["command"], "example");
        let remote = &value["servers"]["remote"]["config"];
        assert_eq!(remote["url"], "https://example.com/mcp");
        assert_eq!(remote["bearer_token_env_var"], "SERVICE_TOKEN");
        assert_eq!(config["tools"]["read"]["output_token_limit"], 512);
        assert_eq!(config["env"]["tokenCount"], "12");
        assert_eq!(config["env"]["tokenBudget"], "2048");
        assert!(!to_yaml(&stack.into()).unwrap().contains("synthetic"));
    }

    #[test]
    fn argument_detection_stops_at_terminator_and_consumes_values_once() {
        let document = r#"
[mcp_servers.example]
command = "example"
args = ["--token", "--password", "ordinary", "--api-key=synthetic-secret", "--", "--token", "positional", "--password=ordinary"]
"#;
        let value = serde_json::to_value(export(document, false).unwrap()).unwrap();
        let args = &value["servers"]["example"]["config"]["args"];
        assert!(args[1]["$env"].is_string());
        assert_eq!(args[2], "ordinary");
        assert!(args[3]["$env"].is_string());
        assert_eq!(args[4], "--");
        assert_eq!(args[5], "--token");
        assert_eq!(args[6], "positional");
        assert_eq!(args[7], "--password=ordinary");
    }

    #[test]
    fn per_secret_decisions_expose_only_selected_values() {
        let document = r#"
[mcp_servers.example]
command = "example"
[mcp_servers.example.env]
FIRST_TOKEN = "first-synthetic-secret"
SECOND_TOKEN = "second-synthetic-secret"
"#;
        let mut seen = Vec::new();
        let stack = export_with_decisions(document, |path, current, total| {
            seen.push((path.to_owned(), current, total));
            Ok(path == "example.env.SECOND_TOKEN")
        })
        .unwrap();
        assert_eq!(
            seen,
            [
                ("example.env.FIRST_TOKEN".to_owned(), 1, 2),
                ("example.env.SECOND_TOKEN".to_owned(), 2, 2)
            ]
        );
        let value = serde_json::to_value(&stack).unwrap();
        assert_eq!(
            value["servers"]["example"]["config"]["env"]["FIRST_TOKEN"],
            serde_json::json!({"$env":"MCPSTACK_EXAMPLE_ENV_FIRST_TOKEN"})
        );
        assert_eq!(
            value["servers"]["example"]["config"]["env"]["SECOND_TOKEN"],
            "second-synthetic-secret"
        );
        assert!(!format!("{seen:?}").contains("synthetic-secret"));
    }

    #[test]
    fn cancelled_selection_returns_no_stack_or_secret_in_error() {
        let document = "[mcp_servers.example]\ncommand='example'\n[mcp_servers.example.env]\nTOKEN='synthetic-secret'\n";
        let result = export_with_decisions(document, |_, _, _| Err(ExportError::Prompt));
        let error = result.err().unwrap();
        assert!(!error.to_string().contains("synthetic-secret"));
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
