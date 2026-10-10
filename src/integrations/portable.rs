//! Convert supported native MCP definitions to the shared stack transport.
use std::collections::BTreeMap;

use serde_json::{Map, Value};

use crate::schema::v1::{EnvironmentReference, Settings, Transport, ValueSource};
use crate::schema::{Server, StackV1};

#[derive(Clone, Copy)]
pub(super) enum Source {
    Codex,
    Claude,
}

pub(super) fn normalize(mut stack: StackV1, source: Source) -> Result<StackV1, String> {
    let mut converted = BTreeMap::new();
    for (name, server) in stack.servers {
        let Server::Configuration { config } = server else {
            converted.insert(name, server);
            continue;
        };
        let value =
            serde_json::to_value(config).map_err(|_| format!("{name:?}: invalid config"))?;
        let mut fields = value
            .as_object()
            .cloned()
            .ok_or_else(|| format!("{name:?}: invalid config"))?;
        let transport = match source {
            Source::Codex => codex_transport(&mut fields, &name)?,
            Source::Claude => claude_transport(&mut fields, &name)?,
        };
        let settings = if matches!(source, Source::Codex) {
            let mut settings = Settings::default();
            if let Some(value) = fields.remove("enabled") {
                settings.enabled = value.as_bool().ok_or_else(|| format!("{name:?}.enabled"))?;
            }
            if let Some(value) = fields.remove("required") {
                settings.required = value
                    .as_bool()
                    .ok_or_else(|| format!("{name:?}.required"))?;
            }
            for (key, slot) in [
                ("startup_timeout_sec", &mut settings.startup_timeout_sec),
                ("tool_timeout_sec", &mut settings.tool_timeout_sec),
            ] {
                if let Some(value) = fields.remove(key) {
                    *slot = Some(value.as_f64().ok_or_else(|| format!("{name:?}.{key}"))?);
                }
            }
            if let Some(value) = fields.remove("enabled_tools") {
                settings.enabled_tools = Some(
                    serde_json::from_value(value).map_err(|_| format!("{name:?}.enabled_tools"))?,
                );
            }
            if let Some(value) = fields.remove("disabled_tools") {
                settings.disabled_tools = serde_json::from_value(value)
                    .map_err(|_| format!("{name:?}.disabled_tools"))?;
            }
            if fields.remove("default_tools_approval_mode").is_some() {
                tracing::warn!(
                    "Omitting {name:?}.default_tools_approval_mode: Claude Code permissions are configured separately."
                );
            }
            settings
        } else {
            Settings::default()
        };
        if let Some(field) = fields.keys().next() {
            return Err(format!("{name:?}.{field:?}"));
        }
        converted.insert(
            name,
            Server::Portable {
                transport,
                settings,
            },
        );
    }
    stack.servers = converted;
    stack
        .validate()
        .map_err(|_| "Portable stack validation failed".to_string())?;
    Ok(stack)
}

fn take_string(
    fields: &mut Map<String, Value>,
    key: &str,
    name: &str,
) -> Result<Option<String>, String> {
    fields
        .remove(key)
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| format!("{name:?}.{key:?}"))
        })
        .transpose()
}

fn take_array(
    fields: &mut Map<String, Value>,
    key: &str,
    name: &str,
    template: bool,
) -> Result<Vec<ValueSource>, String> {
    fields
        .remove(key)
        .map(|value| {
            value
                .as_array()
                .ok_or_else(|| format!("{name:?}.{key:?}"))?
                .iter()
                .cloned()
                .map(|v| source(v, template).ok_or_else(|| format!("{name:?}.{key:?}")))
                .collect()
        })
        .transpose()
        .map(|v| v.unwrap_or_default())
}

fn take_map(
    fields: &mut Map<String, Value>,
    key: &str,
    name: &str,
    template: bool,
) -> Result<BTreeMap<String, ValueSource>, String> {
    fields
        .remove(key)
        .map(|value| {
            value
                .as_object()
                .ok_or_else(|| format!("{name:?}.{key:?}"))?
                .iter()
                .map(|(k, v)| {
                    Ok((
                        k.clone(),
                        source(v.clone(), template).ok_or_else(|| format!("{name:?}.{key:?}"))?,
                    ))
                })
                .collect()
        })
        .transpose()
        .map(|v| v.unwrap_or_default())
}

fn reference(name: &str) -> ValueSource {
    ValueSource::Environment(EnvironmentReference { env: name.into() })
}

fn template(text: &str) -> Option<&str> {
    text.strip_prefix("${")?.strip_suffix('}').filter(|name| {
        !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
    })
}

pub(super) fn is_claude_reference(text: &str) -> bool {
    template(text).is_some() || text.strip_prefix("Bearer ").and_then(template).is_some()
}

fn source(value: Value, recognize_template: bool) -> Option<ValueSource> {
    if let Some(text) = value.as_str() {
        return Some(if recognize_template {
            template(text).map_or_else(|| ValueSource::Literal(text.into()), reference)
        } else {
            ValueSource::Literal(text.into())
        });
    }
    let env = value.as_object()?.get("$env")?.as_str()?;
    if value.as_object()?.len() != 1 {
        return None;
    }
    Some(reference(env))
}

fn codex_transport(fields: &mut Map<String, Value>, name: &str) -> Result<Transport, String> {
    let command = take_string(fields, "command", name)?;
    let url = take_string(fields, "url", name)?;
    match (command, url) {
        (Some(command), None) => {
            let args = take_array(fields, "args", name, false)?;
            let env = take_map(fields, "env", name, false)?;
            let cwd = take_string(fields, "cwd", name)?;
            let env_vars = fields
                .remove("env_vars")
                .map(|v| serde_json::from_value(v).map_err(|_| format!("{name:?}.env_vars")))
                .transpose()?
                .unwrap_or_default();
            Ok(Transport::Stdio {
                command,
                args,
                env,
                env_vars,
                cwd,
            })
        }
        (None, Some(url)) => {
            let mut headers = take_map(fields, "http_headers", name, false)?;
            for (key, value) in take_map(fields, "env_http_headers", name, false)? {
                let ValueSource::Literal(variable) = value else {
                    return Err(format!("{name:?}.env_http_headers"));
                };
                if headers.keys().any(|k| k.eq_ignore_ascii_case(&key)) {
                    return Err(format!("{name:?}.env_http_headers.{key:?}"));
                }
                headers.insert(key, reference(&variable));
            }
            let bearer_token = take_string(fields, "bearer_token_env_var", name)?
                .map(|env| EnvironmentReference { env });
            if bearer_token.is_some()
                && headers
                    .keys()
                    .any(|key| key.eq_ignore_ascii_case("Authorization"))
            {
                return Err(format!(
                    "{name:?}.Authorization: bearer token and header conflict"
                ));
            }
            Ok(Transport::Http {
                url,
                bearer_token,
                headers,
            })
        }
        _ => Err(format!("{name:?}: expected one command or URL")),
    }
}

fn claude_transport(fields: &mut Map<String, Value>, name: &str) -> Result<Transport, String> {
    let kind = take_string(fields, "type", name)?;
    let command = take_string(fields, "command", name)?;
    let url = take_string(fields, "url", name)?;
    match (command, url, kind.as_deref()) {
        (Some(command), None, None | Some("stdio")) => Ok(Transport::Stdio {
            command,
            args: take_array(fields, "args", name, true)?,
            env: take_map(fields, "env", name, true)?,
            env_vars: Vec::new(),
            cwd: None,
        }),
        (None, Some(url), Some("http") | Some("sse")) => {
            let mut headers = take_map(fields, "headers", name, true)?;
            let bearer_token = if let Some(key) = headers
                .keys()
                .find(|key| key.eq_ignore_ascii_case("Authorization"))
                .cloned()
            {
                let bearer = match &headers[&key] {
                    ValueSource::Literal(value) => value
                        .strip_prefix("Bearer ")
                        .and_then(template)
                        .map(str::to_owned),
                    _ => None,
                };
                if bearer.is_some() {
                    headers.remove(&key);
                }
                bearer.map(|env| EnvironmentReference { env })
            } else {
                None
            };
            Ok(if kind.as_deref() == Some("sse") {
                Transport::Sse {
                    url,
                    bearer_token,
                    headers,
                }
            } else {
                Transport::Http {
                    url,
                    bearer_token,
                    headers,
                }
            })
        }
        _ => Err(format!("{name:?}: unsupported Claude transport")),
    }
}
