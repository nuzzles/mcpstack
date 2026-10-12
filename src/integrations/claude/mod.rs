//! Claude Code user-scope MCP configuration in ~/.claude.json.
mod permissions;
use std::collections::BTreeMap;
use std::io::{IsTerminal, Write, stderr, stdin};
use std::path::{Path, PathBuf};

use dialoguer::{Confirm, Select};
use serde_json::{Map, Value};
use thiserror::Error;
use tokio::fs;

use crate::error::{AppError, ErrorCode};
use crate::exporters::to_yaml;
use crate::integrations::secrets::SecretResolver;
use crate::integrations::{Claude, ClientAdapter, ExportArgs, StackArgs};
use crate::integrations::{
    write_dry_run, write_import_result, write_no_changes, write_switch_plan, write_switch_result,
};
use crate::io;
use crate::schema::v1::{ConfigValue, Transport, ValueSource};
use crate::schema::{SCHEMA_VERSION, Server, StackV1};

#[derive(Debug, Error)]
pub enum Error {
    #[error("Cannot determine the Claude Code user configuration path. Supply --config <PATH>.")]
    Path,
    #[error(
        "Invalid Claude Code configuration. Check JSON syntax, the mcpServers object, and permission rule arrays in the selected settings file."
    )]
    Config,
    #[error("This stack contains fields or transports Claude Code cannot represent.")]
    Unsupported,
    #[error(
        "Cannot represent permissions for server {0:?} in Claude Code without loss: {1}. No configuration was changed."
    )]
    Permissions(String, &'static str),
    #[error(
        "Claude Code cannot represent these stack fields: {0}. Edit the stack or use a supported target."
    )]
    UnsupportedFields(String),
    #[error("Server {0:?} defines the same HTTP header in multiple fields.")]
    HeaderConflict(String),
    #[error("Server {0:?} has an invalid environment variable name for an HTTP header.")]
    HeaderEnvironment(String),
    #[error("A masked field needs a nonempty secret value from its environment variable.")]
    Secret,
    #[error(
        "Unable to read Claude Code configuration. Check the selected config/settings path and file permissions."
    )]
    Read,
    #[error("Unable to safely write Claude Code configuration.")]
    Write,
    #[error(transparent)]
    Transaction(io::TransactionError),
    #[error("Unable to create a Claude Code configuration backup.")]
    Backup,
    #[error("Secret selection was cancelled; no stack was exported.")]
    Prompt,
    #[error("Cannot export a portable stack with unsupported field {0}. No stack was exported.")]
    ExportPortable(String),
}
impl Error {
    pub fn code(&self) -> ErrorCode {
        match self {
            Self::Path | Self::Config | Self::Read => ErrorCode::ConfigReadError,
            Self::Unsupported
            | Self::Permissions(_, _)
            | Self::UnsupportedFields(_)
            | Self::HeaderConflict(_)
            | Self::HeaderEnvironment(_)
            | Self::Secret => ErrorCode::ImportError,
            Self::Write | Self::Transaction(_) => ErrorCode::ConfigWriteError,
            Self::Backup => ErrorCode::BackupError,
            Self::Prompt | Self::ExportPortable(_) => ErrorCode::ExportError,
        }
    }
}
impl From<Error> for AppError {
    fn from(error: Error) -> Self {
        Self::Integration(error.into())
    }
}

fn path(config: Option<PathBuf>) -> Result<PathBuf, Error> {
    config
        .or_else(|| directories::BaseDirs::new().map(|dirs| dirs.home_dir().join(".claude.json")))
        .ok_or(Error::Path)
}
fn parse(bytes: &[u8]) -> Result<Value, Error> {
    let value = if bytes.is_empty() {
        Value::Object(Map::new())
    } else {
        serde_json::to_value(
            serde_json::from_slice::<ConfigValue>(bytes).map_err(|_| Error::Config)?,
        )
        .map_err(|_| Error::Config)?
    };
    let object = value.as_object().ok_or(Error::Config)?;
    if let Some(servers) = object.get("mcpServers") {
        servers.as_object().ok_or(Error::Config)?;
    }
    Ok(value)
}
fn validate(definition: &Value) -> Result<(), Error> {
    let object = definition.as_object().ok_or(Error::Config)?;
    if object.keys().any(|key| {
        !matches!(
            key.as_str(),
            "type" | "command" | "args" | "env" | "url" | "headers"
        )
    }) {
        return Err(Error::Unsupported);
    }
    let kind = match object.get("type") {
        Some(Value::String(kind)) => kind.as_str(),
        Some(_) => return Err(Error::Config),
        None if object.contains_key("command") => "stdio",
        None => "",
    };
    match kind {
        "stdio" => {
            if object
                .get("command")
                .and_then(Value::as_str)
                .is_none_or(str::is_empty)
                || object.contains_key("url")
                || object.contains_key("headers")
            {
                return Err(Error::Config);
            }
            if object.get("args").is_some_and(|v| {
                v.as_array()
                    .is_none_or(|a| a.iter().any(|v| !v.is_string()))
            }) || object.get("env").is_some_and(|v| {
                v.as_object()
                    .is_none_or(|m| m.values().any(|v| !v.is_string()))
            }) {
                return Err(Error::Config);
            }
        }
        "http" | "sse" => {
            if object
                .get("url")
                .and_then(Value::as_str)
                .is_none_or(str::is_empty)
                || object.contains_key("command")
                || object.contains_key("args")
                || object.contains_key("env")
            {
                return Err(Error::Config);
            }
            if object.get("headers").is_some_and(|v| {
                v.as_object()
                    .is_none_or(|m| m.values().any(|v| !v.is_string()))
            }) {
                return Err(Error::Config);
            }
        }
        _ => return Err(Error::Unsupported),
    }
    Ok(())
}
fn servers(config: &Value) -> BTreeMap<String, Value> {
    config
        .get("mcpServers")
        .and_then(Value::as_object)
        .map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
        .unwrap_or_default()
}
fn with_servers(mut config: Value, values: &BTreeMap<String, Value>) -> Value {
    config.as_object_mut().unwrap().insert(
        "mcpServers".into(),
        Value::Object(values.iter().map(|(k, v)| (k.clone(), v.clone())).collect()),
    );
    config
}
fn canonical_server(server: &Value) -> Value {
    let mut canonical = server.clone();
    let Some(fields) = canonical.as_object_mut() else {
        return canonical;
    };
    for field in ["args", "env", "headers"] {
        if fields.get(field).is_some_and(|value| {
            value.as_array().is_some_and(Vec::is_empty)
                || value.as_object().is_some_and(Map::is_empty)
        }) {
            fields.remove(field);
        }
    }
    if fields.contains_key("command") && fields.get("type").and_then(Value::as_str) == Some("stdio")
    {
        fields.remove("type");
    }
    canonical
}
fn preserve_equivalent_servers(
    before: &BTreeMap<String, Value>,
    proposed: &mut BTreeMap<String, Value>,
) {
    for (name, definition) in proposed {
        if let Some(current) = before.get(name)
            && canonical_server(current) == canonical_server(definition)
        {
            *definition = current.clone();
        }
    }
}
fn resolve(value: &ConfigValue, resolver: &mut SecretResolver) -> Result<Value, Error> {
    Ok(match value {
        ConfigValue::Object(object) if object.contains_key("$env") => {
            let ConfigValue::String(name) = &object["$env"] else {
                return Err(Error::Unsupported);
            };
            Value::String(resolver.resolve(name).ok_or(Error::Secret)?)
        }
        ConfigValue::Object(object) => Value::Object(
            object
                .iter()
                .map(|(k, v)| Ok((k.clone(), resolve(v, resolver)?)))
                .collect::<Result<_, Error>>()?,
        ),
        ConfigValue::Array(array) => Value::Array(
            array
                .iter()
                .map(|v| resolve(v, resolver))
                .collect::<Result<_, _>>()?,
        ),
        ConfigValue::String(v) => Value::String(v.clone()),
        ConfigValue::Number(v) => Value::Number(v.clone()),
        ConfigValue::Bool(v) => Value::Bool(*v),
        ConfigValue::Null(_) => return Err(Error::Unsupported),
    })
}
fn source(value: &ValueSource, resolver: &mut SecretResolver) -> Result<String, Error> {
    match value {
        ValueSource::Literal(v) => Ok(v.clone()),
        ValueSource::Environment(r) => resolver.resolve(&r.env).ok_or(Error::Secret),
        ValueSource::ResolvedEnvironment(r) => resolver.resolve(&r.value_env).ok_or(Error::Secret),
        ValueSource::PrefixedEnvironment(r) => resolver
            .resolve(&r.env)
            .map(|v| format!("{}{v}", r.prefix))
            .ok_or(Error::Secret),
    }
}
fn environment_reference(name: &str, server: &str) -> Result<String, Error> {
    let mut bytes = name.bytes();
    if !bytes
        .next()
        .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
        || !bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        return Err(Error::HeaderEnvironment(server.into()));
    }
    Ok(format!("${{{name}}}"))
}
fn add_header(
    headers: &mut Map<String, Value>,
    name: &str,
    value: Value,
    server: &str,
) -> Result<(), Error> {
    if headers
        .keys()
        .any(|existing| existing.eq_ignore_ascii_case(name))
    {
        return Err(Error::HeaderConflict(server.into()));
    }
    headers.insert(name.into(), value);
    Ok(())
}
fn map_native_server(server: &str, mut definition: Value) -> Result<Value, Error> {
    let fields = definition.as_object_mut().ok_or(Error::Config)?;
    let direct = fields.remove("http_headers");
    let environment = fields.remove("env_http_headers");
    let bearer = fields.remove("bearer_token_env_var");
    if fields.contains_key("url") && !fields.contains_key("type") {
        fields.insert("type".into(), Value::String("http".into()));
    }
    if direct.is_some() || environment.is_some() || bearer.is_some() {
        let mut headers = match fields.remove("headers") {
            Some(Value::Object(headers)) => headers,
            None => Map::new(),
            Some(_) => return Err(Error::Config),
        };
        if let Some(direct) = direct {
            let direct = direct.as_object().ok_or(Error::Config)?;
            for (name, value) in direct {
                add_header(&mut headers, name, value.clone(), server)?;
            }
        }
        if let Some(environment) = environment {
            let environment = environment.as_object().ok_or(Error::Config)?;
            for (name, variable) in environment {
                let variable = variable.as_str().ok_or(Error::Config)?;
                let reference = environment_reference(variable, server)?;
                add_header(&mut headers, name, Value::String(reference), server)?;
            }
        }
        if let Some(bearer) = bearer {
            let variable = bearer.as_str().ok_or(Error::Config)?;
            let reference = environment_reference(variable, server)?;
            add_header(
                &mut headers,
                "Authorization",
                Value::String(format!("Bearer {reference}")),
                server,
            )?;
        }
        fields.insert("headers".into(), Value::Object(headers));
    }
    Ok(definition)
}
fn unsupported_fields(stack: &StackV1) -> Vec<String> {
    let mut fields = Vec::new();
    for (name, server) in &stack.servers {
        let mut add = |field: &str| fields.push(format!("{name:?}.{field:?}"));
        if crate::integrations::reserved::is_reserved(name) {
            add("server name (reserved by Claude Code)");
        }
        match server {
            Server::Configuration { config } => {
                for key in config.keys() {
                    if !matches!(
                        key.as_str(),
                        "type"
                            | "command"
                            | "args"
                            | "env"
                            | "url"
                            | "headers"
                            | "http_headers"
                            | "env_http_headers"
                            | "bearer_token_env_var"
                    ) {
                        add(key);
                    }
                }
            }
            Server::Portable {
                transport,
                settings,
                ..
            } => {
                if !settings.enabled {
                    add("settings.enabled");
                }
                if settings.required {
                    add("settings.required");
                }
                if settings.startup_timeout_sec.is_some() {
                    add("settings.startup_timeout_sec");
                }
                if settings.tool_timeout_sec.is_some() {
                    add("settings.tool_timeout_sec");
                }
                if settings.enabled_tools.is_some() {
                    add("settings.enabled_tools");
                }
                if !settings.disabled_tools.is_empty() {
                    add("settings.disabled_tools");
                }
                match transport {
                    Transport::Stdio { env_vars, cwd, .. } => {
                        if !env_vars.is_empty() {
                            add("transport.env_vars");
                        }
                        if cwd.is_some() {
                            add("transport.cwd");
                        }
                    }
                    Transport::Http { .. } | Transport::Sse { .. } => {}
                    Transport::Websocket { .. } => add("transport.websocket"),
                }
            }
        }
    }
    fields
}
fn prepare(
    stack: &StackV1,
    resolver: &mut SecretResolver,
) -> Result<BTreeMap<String, Value>, Error> {
    let unsupported = unsupported_fields(stack);
    if !unsupported.is_empty() {
        return Err(Error::UnsupportedFields(unsupported.join(", ")));
    }
    let mut values = BTreeMap::new();
    for (name, server) in &stack.servers {
        let definition = match server {
            Server::Configuration { config } => map_native_server(
                name,
                Value::Object(
                    config
                        .iter()
                        .map(|(k, v)| Ok((k.clone(), resolve(v, resolver)?)))
                        .collect::<Result<_, Error>>()?,
                ),
            )?,
            Server::Portable {
                transport,
                settings,
                ..
            } => {
                if !settings.enabled
                    || settings.required
                    || settings.startup_timeout_sec.is_some()
                    || settings.tool_timeout_sec.is_some()
                    || settings.enabled_tools.is_some()
                    || !settings.disabled_tools.is_empty()
                {
                    return Err(Error::Unsupported);
                }
                match transport {
                    Transport::Stdio {
                        command,
                        args,
                        env,
                        env_vars,
                        cwd,
                        ..
                    } => {
                        if !env_vars.is_empty() || cwd.is_some() {
                            return Err(Error::Unsupported);
                        }
                        let mut definition = serde_json::json!({"type":"stdio", "command":command});
                        let object = definition.as_object_mut().ok_or(Error::Config)?;
                        if !args.is_empty() {
                            object.insert(
                                "args".into(),
                                serde_json::json!(
                                    args.iter()
                                        .map(|v| source(v, resolver))
                                        .collect::<Result<Vec<_>, _>>()?
                                ),
                            );
                        }
                        if !env.is_empty() {
                            object.insert(
                                "env".into(),
                                serde_json::json!(
                                    env.iter()
                                        .map(|(k, v)| Ok((k.clone(), source(v, resolver)?)))
                                        .collect::<Result<BTreeMap<_, _>, Error>>()?
                                ),
                            );
                        }
                        definition
                    }
                    Transport::Http { url, headers } | Transport::Sse { url, headers } => {
                        let kind = if matches!(transport, Transport::Http { .. }) {
                            "http"
                        } else {
                            "sse"
                        };
                        let mut mapped_headers = Map::new();
                        for (header, value) in headers {
                            let text = match value {
                                ValueSource::Literal(value) => value.clone(),
                                ValueSource::Environment(reference) => {
                                    environment_reference(&reference.env, name)?
                                }
                                ValueSource::ResolvedEnvironment(reference) => resolver
                                    .resolve(&reference.value_env)
                                    .ok_or(Error::Secret)?,
                                ValueSource::PrefixedEnvironment(reference) => format!(
                                    "{}{}",
                                    reference.prefix,
                                    environment_reference(&reference.env, name)?
                                ),
                            };
                            add_header(&mut mapped_headers, header, Value::String(text), name)?;
                        }
                        let mut definition = serde_json::json!({"type":kind,"url":url});
                        if !mapped_headers.is_empty() {
                            definition
                                .as_object_mut()
                                .ok_or(Error::Config)?
                                .insert("headers".into(), Value::Object(mapped_headers));
                        }
                        definition
                    }
                    _ => return Err(Error::Unsupported),
                }
            }
        };
        validate(&definition)?;
        values.insert(name.clone(), definition);
    }
    Ok(values)
}
fn prepared(
    stack: &StackV1,
    dry_run: bool,
    interactive: bool,
) -> Result<BTreeMap<String, Value>, AppError> {
    let mut resolver = SecretResolver {
        dry_run,
        interactive,
        preview_missing: false,
        missing_used: false,
        values: BTreeMap::new(),
        cancelled: false,
    };
    let result = prepare(stack, &mut resolver);
    if resolver.cancelled {
        return Err(AppError::ImportApprovalCancelled);
    }
    result.map_err(Into::into)
}
fn prepared_preview(stack: &StackV1) -> Result<(BTreeMap<String, Value>, bool), AppError> {
    let mut resolver = SecretResolver {
        dry_run: true,
        interactive: false,
        preview_missing: true,
        missing_used: false,
        values: BTreeMap::new(),
        cancelled: false,
    };
    let values = prepare(stack, &mut resolver)?;
    Ok((values, resolver.missing_used))
}
fn as_stack(values: BTreeMap<String, Value>) -> Result<StackV1, AppError> {
    let document = serde_json::json!({"schema_version":SCHEMA_VERSION,"servers":values.iter().map(|(k,v)| (k.clone(),serde_json::json!({"config":v}))).collect::<Map<_,_>>()});
    let stack: StackV1 = serde_json::from_value(document).map_err(|_| Error::Config)?;
    stack.validate()?;
    Ok(stack)
}
fn reference_names(stack: &StackV1) -> std::collections::BTreeSet<String> {
    let mut names = std::collections::BTreeSet::new();
    for server in stack.servers.values() {
        if let Server::Configuration { config } = server {
            for value in config.values() {
                super::codex::export::collect_references(value, &mut names);
            }
        }
    }
    names
}
fn mask(values: BTreeMap<String, Value>) -> Result<BTreeMap<String, Value>, AppError> {
    let mut stack = as_stack(values)?;
    let mut names = reference_names(&stack);
    super::codex::export::visit_credentials(&mut stack, &mut names, &mut |_| Ok(false))
        .map_err(|_| Error::Config)?;
    let value = serde_json::to_value(stack).map_err(|_| Error::Config)?;
    Ok(value["servers"]
        .as_object()
        .ok_or(Error::Config)?
        .iter()
        .map(|(name, server)| (name.clone(), server["config"].clone()))
        .collect())
}
fn is_masked(value: &Value) -> bool {
    value
        .as_object()
        .is_some_and(|object| object.contains_key("$env"))
}

fn mark_changed_secrets(
    before: &Value,
    after: &Value,
    before_mask: &mut Value,
    after_mask: &mut Value,
) {
    if is_masked(after_mask) {
        if !is_masked(before_mask) {
            *before_mask = Value::String("<redacted>".into());
        }
        if before != after {
            *after_mask = Value::String("<redacted: changed>".into());
        }
        return;
    }
    match (before, after, before_mask, after_mask) {
        (
            Value::Object(before),
            Value::Object(after),
            Value::Object(before_mask),
            Value::Object(after_mask),
        ) => {
            for (key, after_value) in after {
                if let (Some(before_value), Some(before_mask), Some(after_mask)) = (
                    before.get(key),
                    before_mask.get_mut(key),
                    after_mask.get_mut(key),
                ) {
                    mark_changed_secrets(before_value, after_value, before_mask, after_mask);
                }
            }
        }
        (
            Value::Array(before),
            Value::Array(after),
            Value::Array(before_mask),
            Value::Array(after_mask),
        ) => {
            for (((before, after), before_mask), after_mask) in
                before.iter().zip(after).zip(before_mask).zip(after_mask)
            {
                mark_changed_secrets(before, after, before_mask, after_mask);
            }
        }
        _ => {}
    }
}

fn collect_private_values(source: &Value, masked: &Value, values: &mut BTreeMap<String, String>) {
    if is_masked(masked) || matches!(masked.as_str(), Some("<redacted>" | "<redacted: changed>")) {
        let marker = masked
            .as_str()
            .filter(|text| *text == "<redacted: changed>")
            .unwrap_or("<redacted>");
        if let Some(text) = source.as_str() {
            values.insert(text.into(), marker.into());
        } else if let Some(number) = source.as_number() {
            values.insert(number.to_string(), marker.into());
        }
        return;
    }
    match (source, masked) {
        (Value::Object(source), Value::Object(masked)) => {
            for (key, value) in source {
                if let Some(masked) = masked.get(key) {
                    collect_private_values(value, masked, values);
                }
            }
        }
        (Value::Array(source), Value::Array(masked)) => {
            for (value, masked) in source.iter().zip(masked) {
                collect_private_values(value, masked, values);
            }
        }
        _ => {}
    }
}

struct RootField {
    name: String,
    start: usize,
    value_start: usize,
    value_end: usize,
}

fn skip_space(bytes: &[u8], cursor: &mut usize) {
    while bytes.get(*cursor).is_some_and(u8::is_ascii_whitespace) {
        *cursor += 1;
    }
}

fn string_end(bytes: &[u8], start: usize) -> Result<usize, Error> {
    if bytes.get(start) != Some(&b'"') {
        return Err(Error::Config);
    }
    let mut cursor = start + 1;
    while cursor < bytes.len() {
        match bytes[cursor] {
            b'\\' => cursor += 2,
            b'"' => return Ok(cursor + 1),
            _ => cursor += 1,
        }
    }
    Err(Error::Config)
}

fn value_end(bytes: &[u8], start: usize) -> Result<usize, Error> {
    match bytes.get(start) {
        Some(b'"') => string_end(bytes, start),
        Some(b'{' | b'[') => {
            let mut depth = 0;
            let mut cursor = start;
            while cursor < bytes.len() {
                match bytes[cursor] {
                    b'"' => cursor = string_end(bytes, cursor)?,
                    b'{' | b'[' => {
                        depth += 1;
                        cursor += 1;
                    }
                    b'}' | b']' => {
                        depth -= 1;
                        cursor += 1;
                        if depth == 0 {
                            return Ok(cursor);
                        }
                    }
                    _ => cursor += 1,
                }
            }
            Err(Error::Config)
        }
        Some(_) => {
            let mut cursor = start;
            while bytes
                .get(cursor)
                .is_some_and(|byte| !matches!(byte, b',' | b'}' | b'\n' | b'\r' | b' ' | b'\t'))
            {
                cursor += 1;
            }
            Ok(cursor)
        }
        None => Err(Error::Config),
    }
}

// The configuration has already been parsed and validated. Locate only root
// fields so nested project mcpServers never affect user-scope operations.
fn root_fields(source: &str) -> Result<Vec<RootField>, Error> {
    let bytes = source.as_bytes();
    let mut cursor = 0;
    skip_space(bytes, &mut cursor);
    if bytes.get(cursor) != Some(&b'{') {
        return Err(Error::Config);
    }
    cursor += 1;
    let mut fields = Vec::new();
    loop {
        skip_space(bytes, &mut cursor);
        if bytes.get(cursor) == Some(&b'}') {
            return Ok(fields);
        }
        let start = cursor;
        let key_end = string_end(bytes, start)?;
        let name = serde_json::from_str(&source[start..key_end]).map_err(|_| Error::Config)?;
        cursor = key_end;
        skip_space(bytes, &mut cursor);
        if bytes.get(cursor) != Some(&b':') {
            return Err(Error::Config);
        }
        cursor += 1;
        skip_space(bytes, &mut cursor);
        let value_start = cursor;
        let end = value_end(bytes, cursor)?;
        if fields.iter().any(|field: &RootField| field.name == name) {
            return Err(Error::Config);
        }
        fields.push(RootField {
            name,
            start,
            value_start,
            value_end: end,
        });
        cursor = end;
        skip_space(bytes, &mut cursor);
        match bytes.get(cursor) {
            Some(b',') => cursor += 1,
            Some(b'}') => return Ok(fields),
            _ => return Err(Error::Config),
        }
    }
}

fn proposed_config_text(original: &str, values: &BTreeMap<String, Value>) -> Result<String, Error> {
    if original.is_empty() {
        return serialized_config(&with_servers(Value::Object(Map::new()), values));
    }
    let fields = root_fields(original)?;
    let existing = fields.iter().find(|field| field.name == "mcpServers");
    let indent = existing.map_or(0, |field| {
        let line = &original[original[..field.start]
            .rfind('\n')
            .map_or(0, |index| index + 1)..field.start];
        line.bytes().take_while(|byte| *byte == b' ').count()
    });
    let mut rendered = serde_json::to_string_pretty(values).map_err(|_| Error::Config)?;
    if indent > 0 {
        rendered = rendered.replace('\n', &format!("\n{}", " ".repeat(indent)));
    }
    if let Some(field) = existing {
        let mut output = original.to_owned();
        output.replace_range(field.value_start..field.value_end, &rendered);
        return Ok(output);
    }
    let close = original.rfind('}').ok_or(Error::Config)?;
    let mut output = original.to_owned();
    if let Some(last) = fields.last() {
        output.insert_str(last.value_end, &format!(",\"mcpServers\":{rendered}"));
    } else {
        output.insert_str(close, &format!("\"mcpServers\":{rendered}"));
    }
    Ok(output)
}

/// Replace complete JSON value tokens, including escaped string spellings, without
/// changing physical newlines or accidentally exposing a credential in context.
fn redact_json_values(source: &str, values: &BTreeMap<String, String>) -> String {
    let mut redacted = String::with_capacity(source.len());
    let bytes = source.as_bytes();
    let mut cursor = 0;
    let mut copied = 0;
    while cursor < bytes.len() {
        if bytes[cursor] == b'"' {
            let start = cursor;
            cursor += 1;
            while cursor < bytes.len() {
                if bytes[cursor] == b'\\' {
                    cursor += 2;
                } else if bytes[cursor] == b'"' {
                    cursor += 1;
                    break;
                } else {
                    cursor += 1;
                }
            }
            if cursor > bytes.len() {
                break;
            }
            if let Ok(decoded) = serde_json::from_str::<String>(&source[start..cursor])
                && let Some(marker) = values.get(&decoded)
            {
                redacted.push_str(&source[copied..start]);
                redacted
                    .push_str(&serde_json::to_string(marker).expect("static marker is valid JSON"));
                copied = cursor;
            }
        } else if bytes[cursor] == b'-' || bytes[cursor].is_ascii_digit() {
            let start = cursor;
            while cursor < bytes.len()
                && !matches!(
                    bytes[cursor],
                    b',' | b']' | b'}' | b' ' | b'\t' | b'\r' | b'\n'
                )
            {
                cursor += 1;
            }
            if let Ok(number) = serde_json::from_str::<serde_json::Number>(&source[start..cursor])
                && let Some(marker) = values.get(&number.to_string())
            {
                redacted.push_str(&source[copied..start]);
                redacted
                    .push_str(&serde_json::to_string(marker).expect("static marker is valid JSON"));
                copied = cursor;
            }
        } else {
            cursor += 1;
        }
    }
    redacted.push_str(&source[copied..]);
    redacted
}

fn redact_preview_text(source: &str, values: &BTreeMap<String, String>) -> Result<String, Error> {
    let mut output = String::new();
    let mut copied = 0;
    for field in root_fields(source)? {
        output.push_str(&source[copied..field.start]);
        if field.name == "mcpServers" {
            output.push_str(&source[field.start..field.value_start]);
            output.push_str(&redact_json_values(
                &source[field.value_start..field.value_end],
                values,
            ));
        } else {
            output.push_str("<unrelated setting omitted>");
            for _ in 0..source[field.start..field.value_end]
                .bytes()
                .filter(|byte| *byte == b'\n')
                .count()
            {
                output.push('\n');
            }
        }
        copied = field.value_end;
    }
    output.push_str(&source[copied..]);
    Ok(output)
}

fn serialized_config(config: &Value) -> Result<String, Error> {
    let mut text = serde_json::to_string_pretty(config).map_err(|_| Error::Config)?;
    text.push('\n');
    Ok(text)
}
fn preview(
    output: &mut impl Write,
    path: &Path,
    file: &io::Snapshot,
    before: BTreeMap<String, Value>,
    after: BTreeMap<String, Value>,
    colored: bool,
) -> Result<(), AppError> {
    if before == after {
        return Ok(());
    }
    preview_diff(output, path, file, &before, &after, colored)?;
    summarize_changes(output, path, &before, &after, colored)?;
    Ok(())
}

fn preview_diff(
    output: &mut impl Write,
    path: &Path,
    file: &io::Snapshot,
    before: &BTreeMap<String, Value>,
    after: &BTreeMap<String, Value>,
    colored: bool,
) -> Result<(), AppError> {
    let mut before_mask = mask(before.clone())?;
    let mut after_mask = mask(after.clone())?;
    for (name, after_value) in after {
        if let (Some(before_value), Some(before_mask), Some(after_mask)) = (
            before.get(name),
            before_mask.get_mut(name),
            after_mask.get_mut(name),
        ) {
            mark_changed_secrets(before_value, after_value, before_mask, after_mask);
        }
    }
    let mut before_private = BTreeMap::new();
    let mut after_private = BTreeMap::new();
    for (name, value) in before {
        if let Some(masked) = before_mask.get(name) {
            collect_private_values(value, masked, &mut before_private);
        }
    }
    for (name, value) in after {
        if let Some(masked) = after_mask.get(name) {
            collect_private_values(value, masked, &mut after_private);
        }
    }
    let original = std::str::from_utf8(file.contents()).map_err(|_| Error::Config)?;
    let proposed = proposed_config_text(original, after)?;
    let before_text = if original.is_empty() {
        String::new()
    } else {
        redact_preview_text(original, &before_private)?
    };
    let after_text = redact_preview_text(&proposed, &after_private)?;
    let label = path.to_string_lossy();
    let diff = similar::TextDiff::from_lines(&before_text, &after_text)
        .unified_diff()
        .context_radius(0)
        .header(
            if original.is_empty() {
                "/dev/null"
            } else {
                &label
            },
            &label,
        )
        .to_string();
    writeln!(
        output,
        "Redacted preview; credential placeholders prevent applying this patch directly."
    )?;
    super::diff_output::write_diff(output, &diff, colored)?;
    Ok(())
}

fn summarize_changes(
    output: &mut impl Write,
    path: &Path,
    before: &BTreeMap<String, Value>,
    after: &BTreeMap<String, Value>,
    colored: bool,
) -> Result<(), AppError> {
    let added = after
        .keys()
        .filter(|name| !before.contains_key(*name))
        .cloned()
        .collect();
    let deleted = before
        .keys()
        .filter(|name| !after.contains_key(*name))
        .cloned()
        .collect();
    let modified = after
        .iter()
        .filter(|(name, value)| before.get(*name).is_some_and(|old| old != *value))
        .map(|(name, _)| name.clone())
        .collect();
    super::write_change_summary(output, path, &added, &modified, &deleted, colored)?;
    Ok(())
}
async fn snapshot(
    config: Option<PathBuf>,
    filter: Option<&super::reserved::ServerFilter>,
) -> Result<(PathBuf, io::Snapshot, Value), AppError> {
    let path = path(config)?;
    let file = io::Snapshot::read(&path).await.map_err(|_| Error::Read)?;
    let parsed = parse(file.contents())?;
    let definitions = servers(&parsed);
    if let Some(filter) = filter {
        filter.validate(definitions.keys().map(String::as_str))?;
    }
    for (name, definition) in &definitions {
        if filter.is_none_or(|filter| !filter.skip(name)) {
            validate(definition)?;
        }
    }
    Ok((path, file, parsed))
}
async fn write_with_permissions(
    file: io::Snapshot,
    config: Value,
    config_changed: bool,
    permission_change: Option<(permissions::Snapshot, Value)>,
) -> Result<(), AppError> {
    // Serialize every proposal before backups or writes. Include unchanged files
    // as snapshot guards when only the other file needs updating.
    let bytes = if config_changed {
        let original = std::str::from_utf8(file.contents()).map_err(|_| Error::Write)?;
        Some(
            proposed_config_text(original, &servers(&config))
                .map_err(|_| Error::Write)?
                .into_bytes(),
        )
    } else {
        None
    };
    let mut writes = vec![(file, bytes)];
    if let Some((snapshot, value)) = permission_change {
        let bytes = if snapshot.config != value {
            Some(
                serialized_config(&value)
                    .map_err(|_| Error::Write)?
                    .into_bytes(),
            )
        } else {
            None
        };
        writes.push((snapshot.file, bytes));
    }
    io::replace_all(writes).await.map_err(|error| match error {
        io::TransactionError::Prepare(io::Error::Backup) => Error::Backup,
        error => Error::Transaction(error),
    })?;
    Ok(())
}

impl ClientAdapter for Claude {
    async fn list(
        &self,
        config: Option<PathBuf>,
        filter: super::reserved::ServerFilter,
        output: &mut impl Write,
        colored: bool,
    ) -> Result<(), AppError> {
        let bytes = fs::read(path(config)?)
            .await
            .map_err(AppError::ConfigRead)?;
        let config = if bytes.is_empty() {
            Value::Object(Map::new())
        } else {
            // ConfigValue rejects duplicate keys, including nested definitions.
            serde_json::to_value(
                serde_json::from_slice::<ConfigValue>(&bytes).map_err(|_| AppError::ListConfig)?,
            )
            .map_err(|_| AppError::ListConfig)?
        };
        super::listing::write(
            output,
            &config,
            super::listing::Source::Claude,
            filter,
            colored,
        )
    }

    async fn export(
        &self,
        args: ExportArgs,
        output: &mut impl Write,
        non_interactive: bool,
        expose_secrets: bool,
    ) -> Result<(), AppError> {
        let filter = args.filter();
        let include_permissions = args.includes_permissions();
        let permissions_path = if include_permissions {
            Some(permissions::path(
                args.permissions_config.clone(),
                args.config.as_ref(),
            )?)
        } else {
            None
        };
        let (_, _, config) = snapshot(args.config, Some(&filter)).await?;
        let mut values = servers(&config);
        if include_permissions {
            permissions::validate_names(values.keys().map(String::as_str))?;
        }
        values.retain(|name, _| !filter.skip(name));
        let mut stack = as_stack(values)?;
        if !expose_secrets {
            let mut names = reference_names(&stack);
            let interactive = !non_interactive && stdin().is_terminal() && stderr().is_terminal();
            if interactive {
                let mut total = 0;
                super::codex::export::visit_credentials(&mut stack, &mut names, &mut |_| {
                    total += 1;
                    Ok(true)
                })
                .map_err(|_| Error::Config)?;
                let mut current = 0;
                let mut remaining = None;
                super::codex::export::visit_credentials(&mut stack, &mut names, &mut |field| {
                    current += 1;
                    if let Some(choice) = remaining {
                        return Ok(choice);
                    }
                    let choice = Select::new()
                        .with_prompt(format!(
                            "Secret {current}/{total}: Include {field} as a literal?"
                        ))
                        .items([
                            "No, mask with environment reference",
                            "Yes, include literal value",
                            "No to all, mask this and remaining secrets",
                            "Yes to all, include this and remaining secrets",
                        ])
                        .default(0)
                        .report(false)
                        .interact_opt()
                        .map_err(|_| super::codex::export::ExportError::Prompt)?
                        .ok_or(super::codex::export::ExportError::Prompt)?;
                    if choice >= 2 {
                        remaining = Some(choice == 3);
                    }
                    Ok(choice == 1 || choice == 3)
                })
                .map_err(|_| Error::Prompt)?;
            } else {
                super::codex::export::visit_credentials(&mut stack, &mut names, &mut |_| Ok(false))
                    .map_err(|_| Error::Config)?;
            }
        }
        let mut stack =
            super::portable::normalize(stack, super::portable::Source::Claude, include_permissions)
                .map_err(Error::ExportPortable)?;
        if let Some(path) = permissions_path {
            let settings = permissions::Snapshot::read(&path).await?;
            for (name, server) in &mut stack.servers {
                if let Server::Portable { permissions, .. } = server {
                    *permissions = settings.for_server(name)?.map(Box::new);
                }
            }
            stack.validate()?;
        }
        write!(output, "{}", to_yaml(&stack.into())?)?;
        Ok(())
    }
    async fn diff(
        &self,
        args: StackArgs,
        output: &mut impl Write,
        non_interactive: bool,
        colored: bool,
    ) -> Result<(), AppError> {
        let permission_path = if args.include_permissions {
            Some(permissions::path(
                args.permissions_config.clone(),
                args.config.as_ref(),
            )?)
        } else {
            None
        };
        let (config_path, file, config) = snapshot(args.config, None).await?;
        let stack = args
            .selection
            .read_stack(&args.file, args.include_permissions)
            .await?;
        let before = servers(&config);
        if args.include_permissions {
            permissions::validate_names(
                before
                    .keys()
                    .chain(stack.servers.keys())
                    .map(String::as_str),
            )?;
        }
        let (preview_values, missing) = prepared_preview(&stack)?;
        if missing {
            let mut preliminary = before.clone();
            preliminary.extend(preview_values);
            preserve_equivalent_servers(&before, &mut preliminary);
            if preliminary != before {
                writeln!(
                    output,
                    "Preliminary preview; missing secrets will be requested for an exact comparison."
                )?;
                preview(
                    output,
                    &config_path,
                    &file,
                    before.clone(),
                    preliminary,
                    colored,
                )?;
            }
        }
        let incoming = prepared(
            &stack,
            true,
            !non_interactive && stdin().is_terminal() && stderr().is_terminal(),
        )?;
        let mut proposed = servers(&config);
        let before = proposed.clone();
        proposed.extend(incoming);
        preserve_equivalent_servers(&before, &mut proposed);
        let servers_changed = before != proposed;
        let mut permissions_changed = false;
        // Validate permissions before emitting the final server preview.
        let permission_preview = if let Some(path) = &permission_path {
            let settings = permissions::Snapshot::read(path).await?;
            let names = stack.servers.keys().cloned().collect();
            let proposed = settings.proposed(&stack, &names)?;
            permissions_changed = settings.config != proposed;
            Some((settings, proposed))
        } else {
            None
        };
        if !servers_changed && !permissions_changed {
            write_no_changes(output, &config_path, false)?;
            return Ok(());
        }
        preview(output, &config_path, &file, before, proposed, colored)?;
        if let (Some(path), Some((settings, proposed))) = (permission_path, permission_preview) {
            settings.preview(&proposed, &path, output, colored)?;
        }
        Ok(())
    }
    async fn import(
        &self,
        args: StackArgs,
        output: &mut impl Write,
        non_interactive: bool,
        colored: bool,
        dry_run: bool,
        auto_approve: bool,
    ) -> Result<(), AppError> {
        let permission_path = if args.include_permissions {
            Some(permissions::path(
                args.permissions_config.clone(),
                args.config.as_ref(),
            )?)
        } else {
            None
        };
        let (config_path, file, config) = snapshot(args.config, None).await?;
        let stack = args
            .selection
            .read_stack(&args.file, args.include_permissions)
            .await?;
        let before = servers(&config);
        if args.include_permissions {
            permissions::validate_names(
                before
                    .keys()
                    .chain(stack.servers.keys())
                    .map(String::as_str),
            )?;
        }
        let (preview_values, missing) = prepared_preview(&stack)?;
        if missing {
            let mut preliminary = before.clone();
            preliminary.extend(preview_values);
            preserve_equivalent_servers(&before, &mut preliminary);
            if preliminary != before {
                writeln!(
                    output,
                    "Preliminary changes; missing secrets will be requested for an exact comparison."
                )?;
                summarize_changes(output, &config_path, &before, &preliminary, colored)?;
            }
        }
        let incoming = prepared(
            &stack,
            dry_run,
            (dry_run || !auto_approve)
                && !non_interactive
                && stdin().is_terminal()
                && stderr().is_terminal(),
        )?;
        let mut all_changes = before.clone();
        all_changes.extend(incoming.clone());
        preserve_equivalent_servers(&before, &mut all_changes);
        let permission_snapshot = if let Some(path) = &permission_path {
            Some(permissions::Snapshot::read(path).await?)
        } else {
            None
        };
        let permission_preview = if let Some(settings) = &permission_snapshot {
            let names = stack.servers.keys().cloned().collect();
            Some(settings.proposed(&stack, &names)?)
        } else {
            None
        };
        let permissions_changed = permission_snapshot
            .as_ref()
            .zip(permission_preview.as_ref())
            .is_some_and(|(settings, proposed)| settings.config != *proposed);
        if all_changes == before && !permissions_changed {
            write_no_changes(output, &config_path, dry_run)?;
            return Ok(());
        }
        if let (Some(settings), Some(change), Some(path)) =
            (&permission_snapshot, &permission_preview, &permission_path)
        {
            settings.summarize(change, path, output, colored)?;
        }
        if all_changes != before {
            summarize_changes(output, &config_path, &before, &all_changes, colored)?;
        }
        if !auto_approve && (non_interactive || !stdin().is_terminal() || !stderr().is_terminal()) {
            return Err(AppError::ImportApprovalRequired);
        }
        let mut proposed = before.clone();
        let mut approved = std::collections::BTreeSet::new();
        let total = incoming
            .iter()
            .filter(|(name, definition)| {
                before.get(*name).is_none_or(|existing| {
                    canonical_server(existing) != canonical_server(definition)
                })
            })
            .count();
        let mut current = 0;
        for (name, definition) in incoming {
            if proposed
                .get(&name)
                .is_some_and(|existing| canonical_server(existing) == canonical_server(&definition))
            {
                approved.insert(name);
                continue;
            }
            current += 1;
            if !auto_approve {
                if non_interactive || !stdin().is_terminal() || !stderr().is_terminal() {
                    return Err(AppError::ImportApprovalRequired);
                }
                let action = if before.contains_key(&name) {
                    "Replace existing server"
                } else {
                    "Import"
                };
                if !Confirm::new()
                    .with_prompt(format!("Server {current}/{total}: {action} {name:?}?"))
                    .default(false)
                    .report(false)
                    .interact()
                    .map_err(|_| AppError::ImportApprovalCancelled)?
                {
                    continue;
                }
            }
            approved.insert(name.clone());
            proposed.insert(name, definition);
        }
        let permission_change = if let Some(settings) = permission_snapshot {
            let change = settings.proposed(&stack, &approved)?;
            Some((settings, change))
        } else {
            None
        };
        let permissions_changed = permission_change
            .as_ref()
            .is_some_and(|(settings, change)| settings.config != *change);
        if proposed != all_changes && proposed != before {
            writeln!(output, "Selected server changes:")?;
            summarize_changes(output, &config_path, &before, &proposed, colored)?;
        }
        if proposed == before && !permissions_changed {
            writeln!(
                output,
                "No changes approved; client configuration was not changed."
            )?;
            return Ok(());
        }
        if let (Some((settings, change)), Some(path)) = (&permission_change, &permission_path)
            && permission_preview.as_ref() != Some(change)
        {
            if permissions_changed {
                writeln!(output, "Selected permission changes:")?;
                settings.summarize(change, path, output, colored)?;
            } else {
                writeln!(output, "Selected permission changes: none.")?;
            }
        }
        let imported = proposed
            .iter()
            .filter(|(name, definition)| before.get(*name) != Some(*definition))
            .count();
        if dry_run {
            write_import_result(output, imported, permissions_changed, true)?;
            return Ok(());
        }
        if permissions_changed
            && !auto_approve
            && !Confirm::new()
                .with_prompt("Apply MCP permission changes in Claude Code settings?")
                .default(false)
                .report(false)
                .interact()
                .map_err(|_| AppError::ImportApprovalCancelled)?
        {
            writeln!(
                output,
                "Import cancelled; client configuration and permission settings were not changed."
            )?;
            return Ok(());
        }
        write_with_permissions(
            file,
            with_servers(config, &proposed),
            proposed != before,
            permission_change,
        )
        .await?;
        write_import_result(output, imported, permissions_changed, false)?;
        Ok(())
    }
    async fn use_stack(
        &self,
        args: StackArgs,
        output: &mut impl Write,
        non_interactive: bool,
        colored: bool,
        dry_run: bool,
        auto_approve: bool,
    ) -> Result<(), AppError> {
        let permission_path = if args.include_permissions {
            Some(permissions::path(
                args.permissions_config.clone(),
                args.config.as_ref(),
            )?)
        } else {
            None
        };
        let (config_path, file, config) = snapshot(args.config, None).await?;
        let stack = args
            .selection
            .read_stack(&args.file, args.include_permissions)
            .await?;
        let before = servers(&config);
        if args.include_permissions {
            permissions::validate_names(
                before
                    .keys()
                    .chain(stack.servers.keys())
                    .map(String::as_str),
            )?;
        }
        let (mut preliminary, missing) = prepared_preview(&stack)?;
        if missing {
            preserve_equivalent_servers(&before, &mut preliminary);
            if preliminary != before {
                writeln!(
                    output,
                    "Preliminary changes; missing secrets will be requested for an exact comparison."
                )?;
                summarize_changes(output, &config_path, &before, &preliminary, colored)?;
            }
        }
        let mut proposed = prepared(
            &stack,
            dry_run,
            (dry_run || !auto_approve)
                && !non_interactive
                && stdin().is_terminal()
                && stderr().is_terminal(),
        )?;
        preserve_equivalent_servers(&before, &mut proposed);
        let permission_change = if let Some(path) = &permission_path {
            let settings = permissions::Snapshot::read(path).await?;
            let names = stack.servers.keys().cloned().collect();
            let change = settings.proposed(&stack, &names)?;
            Some((settings, change))
        } else {
            None
        };
        let permissions_changed = permission_change
            .as_ref()
            .is_some_and(|(settings, change)| settings.config != *change);
        if proposed == before && !permissions_changed {
            file.check_unchanged().await.map_err(|_| Error::Write)?;
            write_no_changes(output, &config_path, dry_run)?;
            return Ok(());
        }
        write_switch_plan(output, proposed.len())?;
        if let (Some((settings, change)), Some(path)) = (&permission_change, &permission_path) {
            settings.summarize(change, path, output, colored)?;
        }
        if proposed != before {
            summarize_changes(output, &config_path, &before, &proposed, colored)?;
        }
        if dry_run {
            write_dry_run(output)?;
            return Ok(());
        }
        if !auto_approve {
            if non_interactive || !stdin().is_terminal() || !stderr().is_terminal() {
                return Err(AppError::UseApprovalRequired);
            }
            if !Confirm::new()
                .with_prompt("Replace ALL configured MCP servers with this stack?")
                .default(false)
                .report(false)
                .interact()
                .map_err(|_| AppError::ImportApprovalCancelled)?
            {
                writeln!(
                    output,
                    "Stack switch cancelled; client configuration was not changed."
                )?;
                return Ok(());
            }
        }
        write_with_permissions(
            file,
            with_servers(config, &proposed),
            proposed != before,
            permission_change,
        )
        .await?;
        write_switch_result(output, proposed.len())?;
        Ok(())
    }
}
