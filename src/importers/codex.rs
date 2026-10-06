//! Read-only conversion to the current Codex MCP configuration schema.
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::schema::v1::{Client, ClientValue, Settings, Transport, ValueSource};
use crate::schema::{Server, StackV1};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ImportError {
    #[error(
        "A secret reference is missing, non-Unicode, empty, or contains NUL. Set all referenced environment variables."
    )]
    Secret,
    #[error(
        "The stack contains fields or transports this Codex importer cannot represent. Use supported STDIO or HTTP definitions."
    )]
    Unsupported,
    #[error(
        "Invalid Codex server definition. Check transport fields, settings, and resolved values."
    )]
    Definition,
}

// Core fields used for semantic validation. The bundled schema validates all
// supported fields; native definitions retain their complete original data.
#[derive(Deserialize, Serialize, Default)]
struct Definition {
    #[serde(skip_serializing_if = "Option::is_none")]
    command: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    args: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    env: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    env_vars: Vec<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cwd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    url: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    http_headers: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    env_http_headers: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    bearer_token_env_var: Option<String>,
    #[serde(default = "enabled_default")]
    enabled: bool,
    #[serde(default)]
    required: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    startup_timeout_sec: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_timeout_sec: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    enabled_tools: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    disabled_tools: Vec<String>,
}
fn enabled_default() -> bool {
    true
}

fn secret(
    name: &str,
    lookup: &mut impl FnMut(&str) -> Option<String>,
) -> Result<String, ImportError> {
    lookup(name)
        .filter(|value| !value.is_empty() && !value.contains('\0'))
        .ok_or(ImportError::Secret)
}
fn source(
    value: &ValueSource,
    lookup: &mut impl FnMut(&str) -> Option<String>,
) -> Result<String, ImportError> {
    match value {
        ValueSource::Literal(value) => Ok(value.clone()),
        ValueSource::Environment(reference) => secret(&reference.env, lookup),
    }
}
fn native(
    value: &ClientValue,
    lookup: &mut impl FnMut(&str) -> Option<String>,
) -> Result<serde_json::Value, ImportError> {
    Ok(match value {
        ClientValue::Object(values) if values.contains_key("$env") => {
            let ClientValue::String(name) = &values["$env"] else {
                return Err(ImportError::Definition);
            };
            serde_json::Value::String(secret(name, lookup)?)
        }
        ClientValue::Object(values) => serde_json::Value::Object(
            values
                .iter()
                .map(|(key, value)| Ok((key.clone(), native(value, lookup)?)))
                .collect::<Result<_, ImportError>>()?,
        ),
        ClientValue::Array(values) => serde_json::Value::Array(
            values
                .iter()
                .map(|value| native(value, lookup))
                .collect::<Result<_, _>>()?,
        ),
        ClientValue::String(value) => value.clone().into(),
        ClientValue::Number(value) => value.clone().into(),
        ClientValue::Bool(value) => (*value).into(),
        ClientValue::Null(_) => return Err(ImportError::Unsupported),
    })
}

/// Convert every server before any writes. The caller supplies secret lookup,
/// allowing tests to avoid mutating process-global environment variables.
#[allow(dead_code, reason = "foundation for the stacked import CLI PR")]
pub fn prepare(
    stack: &StackV1,
    mut lookup: impl FnMut(&str) -> Option<String>,
) -> Result<BTreeMap<String, toml::Table>, ImportError> {
    stack.validate().map_err(|_| ImportError::Definition)?;
    stack
        .servers
        .iter()
        .map(|(name, server)| {
            let definition = match server {
                Server::ClientSpecific {
                    client: Client::Codex,
                    config,
                } => {
                    let values = config
                        .iter()
                        .map(|(key, value)| Ok((key.clone(), native(value, &mut lookup)?)))
                        .collect::<Result<serde_json::Map<_, _>, ImportError>>()?;
                    serde_json::Value::Object(values)
                }
                Server::ClientSpecific { .. } => return Err(ImportError::Unsupported),
                Server::Portable {
                    transport,
                    settings,
                } => {
                    let mut definition = Definition {
                        enabled: settings.enabled,
                        required: settings.required,
                        startup_timeout_sec: settings.startup_timeout_sec,
                        tool_timeout_sec: settings.tool_timeout_sec,
                        enabled_tools: settings.enabled_tools.clone(),
                        disabled_tools: settings.disabled_tools.clone(),
                        ..Definition::default()
                    };
                    match transport {
                        Transport::Stdio {
                            command,
                            args,
                            env,
                            env_vars,
                            cwd,
                        } => {
                            definition.command = Some(command.clone());
                            definition.args = args
                                .iter()
                                .map(|value| source(value, &mut lookup))
                                .collect::<Result<_, _>>()?;
                            definition.env = env
                                .iter()
                                .map(|(key, value)| Ok((key.clone(), source(value, &mut lookup)?)))
                                .collect::<Result<_, ImportError>>()?;
                            definition.env_vars = env_vars
                                .iter()
                                .cloned()
                                .map(serde_json::Value::String)
                                .collect();
                            definition.cwd = cwd.clone();
                        }
                        Transport::Http {
                            url,
                            bearer_token,
                            headers,
                        } => {
                            definition.url = Some(url.clone());
                            if let Some(reference) = bearer_token {
                                secret(&reference.env, &mut lookup)?;
                                definition.bearer_token_env_var = Some(reference.env.clone());
                            }
                            definition.http_headers = headers
                                .iter()
                                .map(|(key, value)| Ok((key.clone(), source(value, &mut lookup)?)))
                                .collect::<Result<_, ImportError>>()?;
                        }
                        _ => return Err(ImportError::Unsupported),
                    }
                    serde_json::to_value(definition).map_err(|_| ImportError::Definition)?
                }
            };
            if !crate::adapters::codex::supports(&definition) {
                return Err(ImportError::Unsupported);
            }
            let core: Definition =
                serde_json::from_value(definition.clone()).map_err(|_| ImportError::Definition)?;
            core.validate()?;
            validate_transport_fields(&definition)?;
            let toml::Value::Table(table) =
                toml::Value::try_from(definition).map_err(|_| ImportError::Definition)?
            else {
                return Err(ImportError::Definition);
            };
            Ok((name.clone(), table))
        })
        .collect()
}

fn validate_transport_fields(definition: &serde_json::Value) -> Result<(), ImportError> {
    let incompatible: &[&str] = if definition.get("command").is_some() {
        &[
            "url",
            "http_headers_helper",
            "http_headers",
            "env_http_headers",
            "bearer_token_env_var",
            "oauth",
            "oauth_resource",
            "auth",
        ]
    } else {
        &["command", "args", "env", "env_vars", "cwd"]
    };
    if incompatible
        .iter()
        .any(|field| definition.get(field).is_some())
    {
        return Err(ImportError::Definition);
    }
    if definition
        .get("env_vars")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|values| {
            values.iter().any(|value| {
                value
                    .get("source")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|source| !matches!(source, "local" | "remote"))
            })
        })
    {
        return Err(ImportError::Definition);
    }
    Ok(())
}

impl Definition {
    fn validate(&self) -> Result<(), ImportError> {
        let transport = match (&self.command, &self.url) {
            (Some(command), None)
                if self.http_headers.is_empty()
                    && self.env_http_headers.is_empty()
                    && self.bearer_token_env_var.is_none() =>
            {
                Transport::Stdio {
                    command: command.clone(),
                    args: self
                        .args
                        .iter()
                        .cloned()
                        .map(ValueSource::Literal)
                        .collect(),
                    env: self
                        .env
                        .iter()
                        .map(|(k, v)| (k.clone(), ValueSource::Literal(v.clone())))
                        .collect(),
                    env_vars: self
                        .env_vars
                        .iter()
                        .map(|value| {
                            value
                                .as_str()
                                .or_else(|| value.get("name").and_then(serde_json::Value::as_str))
                                .map(str::to_owned)
                                .ok_or(ImportError::Definition)
                        })
                        .collect::<Result<_, _>>()?,
                    cwd: self.cwd.clone(),
                }
            }
            (None, Some(url))
                if self.args.is_empty()
                    && self.env.is_empty()
                    && self.env_vars.is_empty()
                    && self.cwd.is_none() =>
            {
                // Validate forwarded header variable names without resolving them:
                // Codex itself owns these environment bindings.
                let headers = self
                    .http_headers
                    .iter()
                    .map(|(k, v)| (k.clone(), ValueSource::Literal(v.clone())))
                    .chain(self.env_http_headers.iter().map(|(k, v)| {
                        (
                            k.clone(),
                            ValueSource::Environment(crate::schema::v1::EnvironmentReference {
                                env: v.clone(),
                            }),
                        )
                    }))
                    .collect();
                if self.http_headers.keys().any(|key| {
                    self.env_http_headers
                        .keys()
                        .any(|other| key.eq_ignore_ascii_case(other))
                }) {
                    return Err(ImportError::Definition);
                }
                Transport::Http {
                    url: url.clone(),
                    headers,
                    bearer_token: self
                        .bearer_token_env_var
                        .as_ref()
                        .map(|env| crate::schema::v1::EnvironmentReference { env: env.clone() }),
                }
            }
            _ => return Err(ImportError::Definition),
        };
        let settings = Settings {
            enabled: self.enabled,
            required: self.required,
            startup_timeout_sec: self.startup_timeout_sec,
            tool_timeout_sec: self.tool_timeout_sec,
            enabled_tools: self.enabled_tools.clone(),
            disabled_tools: self.disabled_tools.clone(),
        };
        StackV1 {
            schema_version: 1,
            servers: BTreeMap::from([(
                "check".into(),
                Server::Portable {
                    transport,
                    settings,
                },
            )]),
        }
        .validate()
        .map_err(|_| ImportError::Definition)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn resolves_nested_native_and_portable_references() {
        let stack = StackV1::from_yaml("schema_version: 1\nservers:\n  native:\n    client: codex\n    config:\n      command: tool\n      args: [{'$env': TOKEN}]\n      env: {KEY: {'$env': TOKEN}}\n  portable:\n    transport:\n      type: http\n      url: https://example.com/mcp\n      headers: {Authorization: {env: TOKEN}}\n").unwrap();
        let result = prepare(&stack, |_| Some("fixture-secret".into())).unwrap();
        assert_eq!(
            result["native"]["args"].as_array().unwrap()[0].as_str(),
            Some("fixture-secret")
        );
        assert_eq!(
            result["portable"]["http_headers"]["Authorization"].as_str(),
            Some("fixture-secret")
        );
        for value in [None, Some(String::new()), Some("bad\0value".into())] {
            let error = prepare(&stack, |_| value.clone()).unwrap_err();
            assert_eq!(error, ImportError::Secret);
            assert!(!error.to_string().contains("fixture-secret"));
        }
    }
    #[test]
    fn fails_closed_on_fields_clients_transports_and_resolved_headers() {
        for config in [
            "command: tool\n      future: true",
            "command: tool\n      url: https://example.com",
            "command: 123",
            "url: https://example.com\n      http_headers: {Authorization: {'$env': TOKEN}}",
        ] {
            let stack = StackV1::from_yaml(&format!("schema_version: 1\nservers:\n  test:\n    client: codex\n    config:\n      {config}\n")).unwrap();
            assert!(prepare(&stack, |_| Some("bad\nheader".into())).is_err());
        }
        for transport in ["sse", "websocket"] {
            let scheme = if transport == "websocket" {
                "wss"
            } else {
                "https"
            };
            let stack = StackV1::from_yaml(&format!("schema_version: 1\nservers:\n  test:\n    transport: {{type: {transport}, url: '{scheme}://example.com'}}")).unwrap();
            assert_eq!(
                prepare(&stack, |_| None).unwrap_err(),
                ImportError::Unsupported
            );
        }
    }
    #[test]
    fn export_import_round_trip_preserves_supported_meaning() {
        let original = "[mcp_servers.tool]\ncommand='tool'\nargs=['--token', 'fixture-secret']\n[mcp_servers.tool.env]\nAPI_KEY='fixture-secret'\n";
        let stack = crate::exporters::codex::export(original, false).unwrap();
        let prepared = prepare(&stack, |_| Some("fixture-secret".into())).unwrap();
        assert_eq!(prepared["tool"]["command"].as_str(), Some("tool"));
        assert_eq!(
            prepared["tool"]["env"]["API_KEY"].as_str(),
            Some("fixture-secret")
        );
    }

    #[test]
    fn preserves_current_native_fields_and_rejects_removed_credentials() {
        let original = "[mcp_servers.process]\ncommand='tool'\nenv_vars=[{name='REMOTE_TOKEN',source='remote'}]\n[mcp_servers.process.tools.read]\napproval_mode='prompt'\noutput_token_limit=2000\n[mcp_servers.remote]\nurl='https://example.com/mcp'\nauth='oauth'\nscopes=['read']\n[mcp_servers.remote.oauth]\nclient_id='public-client'\nclient_secret='fixture-secret'\ncallback_port=1234\n";
        let stack = crate::exporters::codex::export(original, false).unwrap();
        let prepared = prepare(&stack, |_| Some("fixture-secret".into())).unwrap();
        let before: toml::Table = toml::from_str(original).unwrap();
        for (name, definition) in before["mcp_servers"].as_table().unwrap() {
            assert_eq!(&toml::Value::Table(prepared[name].clone()), definition);
        }
        let stack = StackV1::from_yaml("schema_version: 1\nservers:\n  remote:\n    client: codex\n    config: {url: 'https://example.com/mcp', bearer_token: fixture-secret}\n").unwrap();
        let error = prepare(&stack, |_| None).unwrap_err();
        assert_eq!(error, ImportError::Unsupported);
        assert!(!error.to_string().contains("fixture-secret"));
    }
}
