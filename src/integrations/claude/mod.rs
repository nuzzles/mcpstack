//! Claude Code user-scope MCP configuration in ~/.claude.json.
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
use crate::io;
use crate::schema::v1::{ConfigValue, Transport, ValueSource};
use crate::schema::{SCHEMA_VERSION, Server, Stack, StackV1};

#[derive(Debug, Error)]
pub enum Error {
    #[error("Cannot determine the Claude Code user configuration path.")]
    Path,
    #[error("Invalid Claude Code user configuration or MCP server definition.")]
    Config,
    #[error("This stack contains fields or transports Claude Code cannot represent.")]
    Unsupported,
    #[error(
        "Claude Code cannot represent these stack fields: {0}. Edit the stack or use a supported target."
    )]
    UnsupportedFields(String),
    #[error("A masked field needs a nonempty secret value from its environment variable.")]
    Secret,
    #[error("Unable to read Claude Code configuration.")]
    Read,
    #[error("Unable to safely write Claude Code configuration.")]
    Write,
    #[error("Unable to create a Claude Code configuration backup.")]
    Backup,
    #[error("Secret selection was cancelled; no stack was exported.")]
    Prompt,
}
impl Error {
    pub fn code(&self) -> ErrorCode {
        match self {
            Self::Path | Self::Config | Self::Read => ErrorCode::ConfigReadError,
            Self::Unsupported | Self::UnsupportedFields(_) | Self::Secret => ErrorCode::ImportError,
            Self::Write => ErrorCode::ConfigWriteError,
            Self::Backup => ErrorCode::BackupError,
            Self::Prompt => ErrorCode::ExportError,
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
        let servers = servers.as_object().ok_or(Error::Config)?;
        for definition in servers.values() {
            validate(definition)?;
        }
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
    }
}
fn unsupported_fields(stack: &StackV1) -> Vec<String> {
    let mut fields = Vec::new();
    for (name, server) in &stack.servers {
        let mut add = |field: &str| fields.push(format!("{name:?}.{field:?}"));
        match server {
            Server::Configuration { config } => {
                for key in config.keys() {
                    if !matches!(
                        key.as_str(),
                        "type" | "command" | "args" | "env" | "url" | "headers"
                    ) {
                        add(key);
                    }
                }
            }
            Server::Portable {
                transport,
                settings,
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
                    Transport::Http { bearer_token, .. } | Transport::Sse { bearer_token, .. } => {
                        if bearer_token.is_some() {
                            add("transport.bearer_token");
                        }
                    }
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
            Server::Configuration { config } => Value::Object(
                config
                    .iter()
                    .map(|(k, v)| Ok((k.clone(), resolve(v, resolver)?)))
                    .collect::<Result<_, Error>>()?,
            ),
            Server::Portable {
                transport,
                settings,
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
                    } => {
                        if !env_vars.is_empty() || cwd.is_some() {
                            return Err(Error::Unsupported);
                        }
                        serde_json::json!({"type":"stdio", "command":command, "args": args.iter().map(|v| source(v, resolver)).collect::<Result<Vec<_>,_>>()?, "env": env.iter().map(|(k,v)| Ok((k.clone(),source(v, resolver)?))).collect::<Result<BTreeMap<_,_>,Error>>()?})
                    }
                    Transport::Http {
                        url,
                        bearer_token,
                        headers,
                    }
                    | Transport::Sse {
                        url,
                        bearer_token,
                        headers,
                    } => {
                        if bearer_token.is_some() {
                            return Err(Error::Unsupported);
                        }
                        let kind = if matches!(transport, Transport::Http { .. }) {
                            "http"
                        } else {
                            "sse"
                        };
                        serde_json::json!({"type":kind,"url":url,"headers":headers.iter().map(|(k,v)| Ok((k.clone(),source(v, resolver)?))).collect::<Result<BTreeMap<_,_>,Error>>()?})
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
        values: BTreeMap::new(),
        cancelled: false,
    };
    let result = prepare(stack, &mut resolver);
    if resolver.cancelled {
        return Err(AppError::ImportApprovalCancelled);
    }
    result.map_err(Into::into)
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
fn preview(
    output: &mut impl Write,
    before: BTreeMap<String, Value>,
    after: BTreeMap<String, Value>,
) -> Result<(), AppError> {
    let mut before_mask = mask(before.clone())?;
    let mut after_mask = mask(after.clone())?;
    for (name, after_value) in &after {
        if let (Some(before_value), Some(before_mask), Some(after_mask)) = (
            before.get(name),
            before_mask.get_mut(name),
            after_mask.get_mut(name),
        ) {
            mark_changed_secrets(before_value, after_value, before_mask, after_mask);
        }
    }
    let before = serde_json::to_string_pretty(&before_mask).map_err(|_| Error::Config)?;
    let after = serde_json::to_string_pretty(&after_mask).map_err(|_| Error::Config)?;
    let diff = similar::TextDiff::from_lines(&before, &after)
        .unified_diff()
        .header("current MCP servers", "proposed MCP servers")
        .to_string();
    write!(output, "{diff}")?;
    Ok(())
}
async fn read_stack(file: &Path) -> Result<StackV1, AppError> {
    let text = fs::read_to_string(file)
        .await
        .map_err(AppError::StackRead)?;
    let Stack::V1(stack) = Stack::from_yaml(&text)?;
    Ok(stack)
}
async fn snapshot(config: Option<PathBuf>) -> Result<(PathBuf, io::Snapshot, Value), AppError> {
    let path = path(config)?;
    let file = io::Snapshot::read(&path).await.map_err(|_| Error::Read)?;
    let parsed = parse(file.contents())?;
    Ok((path, file, parsed))
}
async fn write_config(file: io::Snapshot, config: Value) -> Result<(), AppError> {
    let mut bytes = serde_json::to_vec_pretty(&config).map_err(|_| Error::Write)?;
    bytes.push(b'\n');
    file.replace(&bytes).await.map_err(|_| Error::Write)?;
    Ok(())
}
impl ClientAdapter for Claude {
    async fn export(
        &self,
        args: ExportArgs,
        output: &mut impl Write,
        non_interactive: bool,
        expose_secrets: bool,
    ) -> Result<(), AppError> {
        let (_, _, config) = snapshot(args.config).await?;
        let mut stack = as_stack(servers(&config))?;
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
        write!(output, "{}", to_yaml(&stack.into())?)?;
        Ok(())
    }
    async fn diff(
        &self,
        args: StackArgs,
        output: &mut impl Write,
        non_interactive: bool,
        _colored: bool,
    ) -> Result<(), AppError> {
        let (_, _, config) = snapshot(args.config).await?;
        let stack = read_stack(&args.file).await?;
        let incoming = prepared(
            &stack,
            true,
            !non_interactive && stdin().is_terminal() && stderr().is_terminal(),
        )?;
        let mut proposed = servers(&config);
        let before = proposed.clone();
        proposed.extend(incoming);
        preview(output, before, proposed)
    }
    async fn import(
        &self,
        args: StackArgs,
        output: &mut impl Write,
        non_interactive: bool,
        _colored: bool,
        dry_run: bool,
        auto_approve: bool,
    ) -> Result<(), AppError> {
        let (_, file, config) = snapshot(args.config).await?;
        if !dry_run
            && !auto_approve
            && (non_interactive || !stdin().is_terminal() || !stderr().is_terminal())
        {
            return Err(AppError::ImportApprovalRequired);
        }
        let stack = read_stack(&args.file).await?;
        let incoming = prepared(
            &stack,
            dry_run,
            (dry_run || !auto_approve)
                && !non_interactive
                && stdin().is_terminal()
                && stderr().is_terminal(),
        )?;
        let before = servers(&config);
        let mut proposed = before.clone();
        for (name, definition) in incoming {
            if proposed.get(&name) == Some(&definition) {
                continue;
            }
            if !auto_approve {
                if non_interactive || !stdin().is_terminal() || !stderr().is_terminal() {
                    return Err(AppError::ImportApprovalRequired);
                }
                if !Confirm::new()
                    .with_prompt(format!("Import or replace {name:?}?"))
                    .default(false)
                    .interact()
                    .map_err(|_| AppError::ImportApprovalCancelled)?
                {
                    continue;
                }
            }
            proposed.insert(name, definition);
        }
        if proposed == before {
            writeln!(
                output,
                "No changes; existing identical entries were unchanged."
            )?;
            return Ok(());
        }
        if dry_run {
            preview(output, before, proposed)?;
            return Ok(());
        }
        file.ensure_write_supported().map_err(|_| Error::Write)?;
        let backup = file
            .default_backup_path()
            .await
            .map_err(|_| Error::Backup)?;
        file.create_backup_at(&backup)
            .await
            .map_err(|_| Error::Backup)?;
        write_config(file, with_servers(config, &proposed)).await?;
        writeln!(output, "Imported MCP servers.")?;
        Ok(())
    }
    async fn use_stack(
        &self,
        args: StackArgs,
        output: &mut impl Write,
        non_interactive: bool,
        _colored: bool,
        dry_run: bool,
        auto_approve: bool,
    ) -> Result<(), AppError> {
        let (_, file, config) = snapshot(args.config).await?;
        let stack = read_stack(&args.file).await?;
        let proposed = prepared(
            &stack,
            dry_run,
            (dry_run || !auto_approve)
                && !non_interactive
                && stdin().is_terminal()
                && stderr().is_terminal(),
        )?;
        let before = servers(&config);
        if proposed == before {
            file.check_unchanged().await.map_err(|_| Error::Write)?;
            writeln!(
                output,
                "No changes; the configured MCP servers already match this stack."
            )?;
            return Ok(());
        }
        if dry_run {
            preview(output, before, proposed)?;
            return Ok(());
        }
        if !auto_approve {
            if non_interactive || !stdin().is_terminal() || !stderr().is_terminal() {
                return Err(AppError::UseApprovalRequired);
            }
            if !Confirm::new()
                .with_prompt("Replace ALL configured MCP servers with this stack?")
                .default(false)
                .interact()
                .map_err(|_| AppError::ImportApprovalCancelled)?
            {
                writeln!(output, "Stack switch cancelled.")?;
                return Ok(());
            }
        }
        file.ensure_write_supported().map_err(|_| Error::Write)?;
        let backup = file
            .default_backup_path()
            .await
            .map_err(|_| Error::Backup)?;
        file.create_backup_at(&backup)
            .await
            .map_err(|_| Error::Backup)?;
        write_config(file, with_servers(config, &proposed)).await?;
        writeln!(output, "Using stack with {} MCP server(s).", proposed.len())?;
        Ok(())
    }
}
