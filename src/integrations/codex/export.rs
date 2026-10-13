use std::collections::{BTreeMap, BTreeSet};

use thiserror::Error;

use crate::schema::{SCHEMA_VERSION, Server, StackV1, v1::ConfigValue};

/// Diagnostics deliberately omit parser contents, field values.
#[derive(Debug, Error)]
pub enum ExportError {
    #[error(transparent)]
    Selection(#[from] crate::integrations::selection::Error),
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
    #[error("Cannot export a portable stack with unsupported field {0}. No stack was exported.")]
    Portable(String),
}

/// Export one native entry per server. This reads data only: it does not resolve
/// credentials, run helpers, or infer installed-client compatibility.
#[cfg(test)]
pub fn export(document: &str, expose_secrets: bool) -> Result<StackV1, ExportError> {
    export_filtered(document, expose_secrets, Default::default())
}

pub fn export_filtered(
    document: &str,
    expose_secrets: bool,
    filter: crate::integrations::reserved::ServerFilter,
) -> Result<StackV1, ExportError> {
    export_with_filter(document, filter, |_, _, _| Ok(expose_secrets))
}

/// Decide whether to expose each detected credential. The callback receives
/// only a field path and its one-based position and total, never the value.
#[cfg(test)]
pub fn export_with_decisions(
    document: &str,
    expose: impl FnMut(&str, usize, usize) -> Result<bool, ExportError>,
) -> Result<StackV1, ExportError> {
    export_with_filter(document, Default::default(), expose)
}

pub fn export_with_filter(
    document: &str,
    filter: crate::integrations::reserved::ServerFilter,
    mut expose: impl FnMut(&str, usize, usize) -> Result<bool, ExportError>,
) -> Result<StackV1, ExportError> {
    let config: toml::Value = toml::from_str(document).map_err(|_| ExportError::Config)?;
    let servers = match config.get("mcp_servers") {
        None => {
            filter.validate(std::iter::empty())?;
            serde_json::Map::new()
        }
        Some(toml::Value::Table(table)) => {
            filter.validate(table.keys().map(String::as_str))?;
            let mut table = table.clone();
            let reserved: Vec<_> = table
                .keys()
                .filter(|name| filter.skip(name))
                .cloned()
                .collect();
            for name in reserved {
                table.remove(&name);
            }
            for (_, server) in table.iter_mut() {
                if let toml::Value::Table(fields) = server {
                    fields.remove("sandbox_mode");
                    fields.remove("approval_policy");
                }
            }
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
    if !servers.values().all(super::schema::supports) {
        return Err(ExportError::Schema);
    }
    for definition in servers.values() {
        crate::integrations::header_helpers::validate_native(definition)
            .map_err(|_| ExportError::Schema)?;
    }
    let definitions: BTreeMap<_, _> = servers
        .into_iter()
        .map(|(name, config)| (name, serde_json::json!({"config":config})))
        .collect();
    let document = serde_json::json!({"schema_version":SCHEMA_VERSION,"servers":definitions});
    let mut stack: StackV1 = serde_json::from_value(document).map_err(|_| ExportError::Stack)?;
    stack.validate().map_err(|_| ExportError::Stack)?;
    let mut names = BTreeSet::new();
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

/// Redact a private section for a preview, including existing fields outside
/// the supported import schema. This does not validate or transform config.
pub fn mask_for_preview(
    name: &str,
    definition: &toml::Table,
) -> Result<serde_json::Value, ExportError> {
    let document = serde_json::json!({"schema_version": SCHEMA_VERSION, "servers": {
        name: {"config": definition}
    }});
    let mut stack: StackV1 = serde_json::from_value(document).map_err(|_| ExportError::Config)?;
    let mut names = BTreeSet::new();
    visit_credentials(&mut stack, &mut names, &mut |_| Ok(false))?;
    let value = serde_json::to_value(stack).map_err(|_| ExportError::Config)?;
    Ok(value["servers"][name]["config"].clone())
}

pub(crate) fn visit_credentials(
    stack: &mut StackV1,
    names: &mut BTreeSet<String>,
    expose: &mut impl FnMut(&str) -> Result<bool, ExportError>,
) -> Result<(), ExportError> {
    // Reserve bindings across the entire selection before masking any server.
    // Runtime credentials must never satisfy another field's generated reference.
    // Fold reservation names to uppercase on every platform so exported stacks
    // also avoid case-insensitive environment-name collisions on Windows.
    for server in stack.servers.values() {
        if let Server::Configuration { config } = server {
            for value in config.values() {
                collect_references(value, names);
            }
            if let Some(ConfigValue::String(name)) = config.get("bearer_token_env_var") {
                names.insert(name.to_ascii_uppercase());
            }
            if let Some(ConfigValue::Object(headers)) = config.get("env_http_headers") {
                for value in headers.values() {
                    if let ConfigValue::String(name) = value {
                        names.insert(name.to_ascii_uppercase());
                    }
                }
            }
            if let Some(ConfigValue::Array(variables)) = config.get("env_vars") {
                for variable in variables {
                    let name = match variable {
                        ConfigValue::String(name) => Some(name),
                        ConfigValue::Object(fields) => match fields.get("name") {
                            Some(ConfigValue::String(name)) => Some(name),
                            _ => None,
                        },
                        _ => None,
                    };
                    if let Some(name) = name {
                        names.insert(name.to_ascii_uppercase());
                    }
                }
            }
        }
    }
    for (server_name, server) in &mut stack.servers {
        if let Server::Configuration { config, .. } = server {
            for (key, value) in config {
                if key == "env_http_headers" {
                    continue;
                }
                let sensitive = secret_key(key)
                    || (matches!(key.as_str(), "headersHelper" | "http_headers_helper")
                        && matches!(value, ConfigValue::String(command) if helper_has_credentials(command)));
                protect(
                    value,
                    &format!("{server_name}_{key}"),
                    &format!("{server_name}.{key}"),
                    sensitive,
                    key == "args",
                    names,
                    expose,
                )?;
            }
        }
    }
    Ok(())
}

fn collect_references(value: &ConfigValue, names: &mut BTreeSet<String>) {
    match value {
        ConfigValue::Object(values) => {
            if let Some(ConfigValue::String(name)) = values.get("$env") {
                names.insert(name.to_ascii_uppercase());
            } else {
                for value in values.values() {
                    collect_references(value, names);
                }
            }
        }
        ConfigValue::Array(values) => {
            for value in values {
                collect_references(value, names);
            }
        }
        ConfigValue::String(text) => {
            // Claude expands braced variables, including prefixed/interpolated
            // expressions and defaults. Reserve names without expanding values.
            for (start, _) in text.match_indices("${") {
                if let Some((expression, _)) = text[start + 2..].split_once('}') {
                    let name = expression
                        .split_once(":-")
                        .map_or(expression, |(name, _)| name);
                    if helper_variable(name) {
                        names.insert(name.to_ascii_uppercase());
                    }
                }
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

// Recognize the same named credentials in opaque shell commands: --token,
// TOKEN=..., and JSON/header forms such as "Authorization": ... . Mask the
// whole command rather than attempting to rewrite shell syntax. Script paths
// and ordinary commands (including get-token) are not themselves credentials.
fn helper_has_credentials(command: &str) -> bool {
    let mut start = None;
    let mut quote = None;
    let mut escaped = false;
    for (end, character) in command
        .char_indices()
        .chain(std::iter::once((command.len(), '\0')))
    {
        if character.is_ascii_alphanumeric() || matches!(character, '_' | '-') {
            start.get_or_insert(end);
        } else if let Some(begin) = start.take() {
            let word = &command[begin..end];
            let after = trim_helper_closing_quotes(&command[end..]).trim_start();
            let value = if let Some(value) = after.strip_prefix([':', '=']) {
                Some(value)
            } else if word.starts_with('-') {
                Some(after)
            } else {
                None
            };
            if secret_key(word.trim_start_matches('-'))
                && value.is_some_and(|value| {
                    !helper_runtime_value(value, quote, after.starts_with(':'))
                })
            {
                return true;
            }
        }
        if escaped {
            escaped = false;
        } else if character == '\\' && quote != Some('\'') {
            escaped = true;
        } else if Some(character) == quote {
            quote = None;
        } else if quote.is_none() && matches!(character, '\'' | '"') {
            quote = Some(character);
        }
    }
    false
}

// Inspect common shell references without expanding or running anything. An
// exemption applies only to this credential value, never to the whole command:
// other literal credentials (including inside substitutions) are still scanned.
fn helper_runtime_value(value: &str, enclosing_quote: Option<char>, header_value: bool) -> bool {
    let value = value.trim_start();
    let opening_quote = value
        .strip_prefix('\\')
        .unwrap_or(value)
        .chars()
        .next()
        .filter(|c| matches!(c, '\'' | '"'));
    // Single-quoted shell arguments do not expand runtime credentials.
    if opening_quote == Some('\'') {
        return false;
    }
    let closing_quote = opening_quote.or(enclosing_quote);
    let value = trim_helper_quotes(value);
    let value = value
        .strip_prefix("Bearer ")
        .or_else(|| value.strip_prefix("Basic "))
        .map(trim_helper_quotes)
        .unwrap_or(value);
    let end = if let Some(variable) = value.strip_prefix("${") {
        variable
            .find('}')
            .filter(|end| helper_variable(&variable[..*end]))
            .map(|end| end + 3)
    } else if value.starts_with("$(") {
        helper_substitution_end(value)
    } else if let Some(command) = value.strip_prefix('`') {
        command.find('`').map(|end| end + 2)
    } else if let Some(variable) = value
        .strip_prefix("$env:")
        .or_else(|| value.strip_prefix('$'))
    {
        let length = variable
            .bytes()
            .take_while(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
            .count();
        helper_variable(&variable[..length]).then_some(value.len() - variable.len() + length)
    } else if value.starts_with('%') || value.starts_with('!') {
        let variable = &value[1..];
        variable
            .find(char::from(value.as_bytes()[0]))
            .filter(|end| helper_variable(&variable[..*end]))
            .map(|end| end + 2)
    } else {
        None
    };
    let Some(end) = end else {
        return false;
    };
    let remainder = &value[end..];
    if closing_quote.is_some_and(|quote| {
        !remainder.starts_with(quote)
            && !remainder
                .strip_prefix('\\')
                .is_some_and(|s| s.starts_with(quote))
    }) {
        return false;
    }
    // A literal suffix concatenated to a reference can itself contain a secret.
    // Quotes may close a shell word or a JSON string, but do not end a word by themselves.
    let trimmed = trim_helper_closing_quotes(remainder);
    trimmed.is_empty()
        || trimmed.starts_with(|c: char| c.is_whitespace() || ";|&".contains(c))
        || (header_value
            && trimmed.len() < remainder.len()
            && trimmed.starts_with([',', ')', '}', ']']))
}

fn helper_variable(name: &str) -> bool {
    let mut bytes = name.bytes();
    bytes
        .next()
        .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

fn trim_helper_quotes(mut value: &str) -> &str {
    loop {
        let trimmed = trim_helper_closing_quotes(value.trim_start());
        if trimmed == value {
            return value;
        }
        value = trimmed;
    }
}

fn trim_helper_closing_quotes(mut value: &str) -> &str {
    loop {
        if let Some(rest) = value.strip_prefix('"').or_else(|| value.strip_prefix('\'')) {
            value = rest;
        } else if let Some(rest) = value
            .strip_prefix("\\\"")
            .or_else(|| value.strip_prefix("\\'"))
        {
            value = rest;
        } else {
            return value;
        }
    }
}

fn helper_substitution_end(value: &str) -> Option<usize> {
    let mut depth = 0;
    let mut quote = None;
    let mut escaped = false;
    for (index, c) in value.char_indices().skip(1) {
        if escaped {
            escaped = false;
            continue;
        }
        if c == '\\' && quote != Some('\'') {
            escaped = true;
            continue;
        }
        if let Some(current) = quote {
            if c == current {
                quote = None;
            }
            continue;
        }
        match c {
            '\'' | '"' => quote = Some(c),
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(index + 1);
                }
            }
            _ => {}
        }
    }
    None
}

fn protect(
    value: &mut ConfigValue,
    path: &str,
    display_path: &str,
    sensitive: bool,
    arguments: bool,
    names: &mut BTreeSet<String>,
    expose: &mut impl FnMut(&str) -> Result<bool, ExportError>,
) -> Result<(), ExportError> {
    match value {
        ConfigValue::Object(values) => {
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
        ConfigValue::Array(values) => {
            let mut next_secret = false;
            let mut options = arguments;
            for (index, value) in values.iter_mut().enumerate() {
                if options
                    && !next_secret
                    && matches!(value, ConfigValue::String(text) if text == "--")
                {
                    options = false;
                }
                let (inline_secret, following_secret) = if options && !next_secret {
                    match value {
                        ConfigValue::String(text) if text.starts_with('-') => {
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
        ConfigValue::String(text)
            if sensitive && crate::integrations::portable::is_claude_reference(text) => {}
        ConfigValue::String(_) | ConfigValue::Number(_) if sensitive => {
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
            *value = ConfigValue::Object(BTreeMap::from([(
                "$env".to_owned(),
                ConfigValue::String(name),
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

    #[test]
    fn helper_runtime_values_do_not_hide_literal_credentials_elsewhere() {
        for command in [
            r#"auth --token "$TOKEN" --profile work"#,
            r#"auth --token="${TOKEN}""#,
            "TOKEN=$(get-token) auth",
            r#"auth --token "$(get-token --profile work)""#,
            r#"auth --token "$(get-token "$(profile)")""#,
            "auth --token `get-token`",
            "auth --token %TOKEN%",
            "auth --token !TOKEN!",
            "auth --token $env:TOKEN",
            r#"auth -H "Authorization: Bearer ${TOKEN}""#,
            r#"echo '{"Authorization": "Bearer '"$(get-token)"'"}'"#,
        ] {
            assert!(
                !helper_has_credentials(command),
                "runtime-only command was masked: {command}"
            );
        }
        for command in [
            r#"auth --token "$TOKEN" --password literal-fixture-secret"#,
            r#"auth --token "$(get-token --password literal-fixture-secret)""#,
            r#"auth --token "${TOKEN}literal-fixture-secret""#,
            r#"auth --token "${TOKEN}"literal-fixture-secret"#,
            r#"auth --token "${TOKEN} literal-fixture-secret""#,
            r#"auth --token ${TOKEN}]literal-fixture-secret"#,
            r#"auth --token "${TOKEN}",literal-fixture-secret"#,
            r#"auth --token "${TOKEN:-literal-fixture-secret}""#,
            r#"auth -H "Authorization: Bearer ${TOKEN} literal-fixture-secret""#,
            r#"echo '{"Authorization":"Bearer literal-fixture-secret"}'; auth --token "$TOKEN""#,
            r#"auth --token '\$TOKEN'"#,
            r#"auth --token "\$TOKEN""#,
            "auth --token %TOKEN%literal-fixture-secret",
        ] {
            assert!(
                helper_has_credentials(command),
                "literal credential was exposed: {command}"
            );
        }
    }

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
    fn ignores_server_policy_fields_but_still_rejects_other_unknown_fields() {
        let config = r#"
[mcp_servers.notion]
url = "https://example.com/mcp"
sandbox_mode = "workspace-write"
approval_policy = "on-request"
[mcp_servers.local]
command = "example"
"#;
        let stack = export(config, false).unwrap();
        let value = serde_json::to_value(stack).unwrap();
        assert_eq!(
            value["servers"]["notion"]["config"]["url"],
            "https://example.com/mcp"
        );
        assert!(
            value["servers"]["notion"]["config"]
                .get("sandbox_mode")
                .is_none()
        );
        assert!(
            value["servers"]["notion"]["config"]
                .get("approval_policy")
                .is_none()
        );
        assert_eq!(value["servers"]["local"]["config"]["command"], "example");
        assert!(matches!(
            export(&format!("{config}\nfuture_field = true\n"), false),
            Err(ExportError::Schema)
        ));
        assert!(export(&config.replace("\"workspace-write\"", "2020-01-01"), false).is_ok());
    }

    #[test]
    fn skips_reserved_names_before_validating_or_exporting_servers() {
        let mut config = String::new();
        for name in [
            "workspace",
            "claude-in-chrome",
            "computer-use",
            "Claude Preview",
            "Claude Browser",
        ] {
            config.push_str(&format!(
                "[mcp_servers.{name:?}]\ncommand = 'built-in'\nunknown_field = true\n"
            ));
        }
        config.push_str("[mcp_servers.computer-use-extra]\ncommand = 'tool'\n");
        let exported = serde_json::to_value(export(&config, false).unwrap()).unwrap();
        assert_eq!(
            exported["servers"]
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            vec!["computer-use-extra"]
        );
    }

    #[test]
    fn node_repl_is_skipped_unless_allowed() {
        let config = "[mcp_servers.node_repl]\ncommand = 'node'\nunknown_field = true\n";
        assert!(export(config, false).unwrap().servers.is_empty());
        assert!(matches!(
            export_filtered(
                config,
                false,
                crate::integrations::reserved::ServerFilter {
                    node_repl: true,
                    ..Default::default()
                }
            ),
            Err(ExportError::Schema)
        ));
        let config = "[mcp_servers.node_repl]\ncommand = 'node'\n";
        assert!(
            export_filtered(
                config,
                false,
                crate::integrations::reserved::ServerFilter {
                    node_repl: true,
                    ..Default::default()
                }
            )
            .unwrap()
            .servers
            .contains_key("node_repl")
        );
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
        let mut stack = StackV1::from_yaml("schema_version: 1\nservers:\n  x:\n    config:\n      api-key: first-secret\n      api_key: second-secret\n      pin: 1234\n      enabled: true\n      existing: {'$env': MCPSTACK_X_API_KEY}\n").unwrap();
        let mut names = BTreeSet::new();
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
    fn runtime_bindings_reserve_names_across_servers_and_case() {
        let reference = "mcpstack_demo_api_key";
        for binding in [
            serde_json::json!({"existing": {"$env": reference}}),
            serde_json::json!({"bearer_token_env_var": reference}),
            serde_json::json!({"env_http_headers": {"Authorization": reference}}),
            serde_json::json!({"env_vars": [reference]}),
            serde_json::json!({"env_vars": [{"name": reference, "default": "fallback"}]}),
            serde_json::json!({"args": [format!("prefix-${{{reference}}}-suffix")]}),
            serde_json::json!({"args": [format!("${{{reference}:-fallback}}")]}),
            serde_json::json!({"args": [format!("${{OUTER:-${{{reference}}}}}")]}),
        ] {
            let mut stack: StackV1 = serde_json::from_value(serde_json::json!({
                "schema_version": SCHEMA_VERSION,
                "servers": {
                    "demo": {"config": {"api_key": "literal-fixture-secret"}},
                    "later": {"config": binding.clone()}
                }
            }))
            .unwrap();
            visit_credentials(&mut stack, &mut BTreeSet::new(), &mut |_| Ok(false)).unwrap();
            let value = serde_json::to_value(stack).unwrap();
            assert_eq!(
                value["servers"]["demo"]["config"]["api_key"]["$env"], "MCPSTACK_DEMO_API_KEY_2",
                "{binding}"
            );
            assert_eq!(value["servers"]["later"]["config"], binding);
        }
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
