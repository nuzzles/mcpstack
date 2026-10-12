//! Read-only inventory shared by the client adapters. Only displayed fields are
//! validated; unrelated or newer client settings do not block an inventory.
use std::collections::BTreeMap;
use std::io::Write;

use serde_json::Value;

use crate::error::AppError;

#[derive(Clone, Copy)]
pub(super) enum Source {
    Codex,
    Claude,
}

pub(super) fn write(
    output: &mut impl Write,
    config: &Value,
    source: Source,
    filter: super::reserved::ServerFilter,
    colored: bool,
) -> Result<(), AppError> {
    let root = config.as_object().ok_or(AppError::ListConfig)?;
    let key = match source {
        Source::Codex => "mcp_servers",
        Source::Claude => "mcpServers",
    };
    let definitions: BTreeMap<_, _> = match root.get(key) {
        None => BTreeMap::new(),
        Some(value) => value
            .as_object()
            .ok_or(AppError::ListConfig)?
            .iter()
            .collect(),
    };
    filter.validate(definitions.keys().map(|name| name.as_str()))?;
    // Prepare every row before writing, so invalid entries produce no partial list.
    let rows = definitions
        .into_iter()
        .filter(|(name, _)| !filter.skip(name))
        .map(|(name, value)| row(name, value, source))
        .collect::<Result<Vec<_>, _>>()?;
    if rows.is_empty() {
        writeln!(output, "No MCP servers configured.")?;
        return Ok(());
    }
    let headers = ["NAME", "TRANSPORT", "STATE", "COMMAND / ENDPOINT"];
    let widths: Vec<_> = headers
        .iter()
        .enumerate()
        .map(|(i, heading)| {
            rows.iter()
                .map(|row| row[i].chars().count())
                .chain([heading.len()])
                .max()
                .unwrap()
        })
        .collect();
    write_row(output, &headers.map(str::to_owned), &widths, colored, true)?;
    for row in &rows {
        write_row(output, row, &widths, colored, false)?;
    }
    Ok(())
}

fn row(name: &str, definition: &Value, source: Source) -> Result<[String; 4], AppError> {
    let fields = definition.as_object().ok_or(AppError::ListConfig)?;
    let command = text(fields.get("command"))?;
    let url = text(fields.get("url"))?;
    let transport = match source {
        Source::Codex => match (command, url) {
            (Some(_), None) => "stdio",
            (None, Some(_)) => "http",
            _ => return Err(AppError::ListConfig),
        },
        Source::Claude => match text(fields.get("type"))? {
            None | Some("stdio") if command.is_some() && url.is_none() => "stdio",
            Some(kind @ ("http" | "sse")) if url.is_some() && command.is_none() => kind,
            _ => return Err(AppError::ListConfig),
        },
    };
    let state = match source {
        Source::Codex => match fields.get("enabled") {
            None | Some(Value::Bool(true)) => "enabled",
            Some(Value::Bool(false)) => "disabled",
            _ => return Err(AppError::ListConfig),
        },
        Source::Claude if super::reserved::is_reserved(name) => "ignored",
        Source::Claude => "configured",
    };
    let endpoint = match (command, url) {
        (Some(command), None) => command.to_owned(),
        (None, Some(value)) => safe_url(value)?,
        _ => return Err(AppError::ListConfig),
    };
    Ok([
        cell(name),
        transport.to_owned(),
        state.to_owned(),
        cell(&endpoint),
    ])
}

fn text(value: Option<&Value>) -> Result<Option<&str>, AppError> {
    value
        .map(|value| {
            value
                .as_str()
                .filter(|v| !v.trim().is_empty())
                .ok_or(AppError::ListConfig)
        })
        .transpose()
}

fn safe_url(value: &str) -> Result<String, AppError> {
    let mut url = url::Url::parse(value).map_err(|_| AppError::ListConfig)?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err(AppError::ListConfig);
    }
    // Credentials can occur in URL user info, query values, or fragments.
    url.set_username("").map_err(|_| AppError::ListConfig)?;
    url.set_password(None).map_err(|_| AppError::ListConfig)?;
    url.set_query(None);
    url.set_fragment(None);
    Ok(url.into())
}

fn cell(value: &str) -> String {
    value.escape_debug().to_string()
}

fn write_row(
    output: &mut impl Write,
    row: &[String; 4],
    widths: &[usize],
    colored: bool,
    header: bool,
) -> std::io::Result<()> {
    for (index, value) in row.iter().enumerate() {
        let style = if header || index == 0 {
            Some(1) // bold headings and server names
        } else if index == 2 {
            match value.as_str() {
                "enabled" => Some(32),              // green
                "disabled" | "ignored" => Some(33), // yellow
                "configured" => Some(36),           // cyan
                _ => None,
            }
        } else {
            None
        };
        if let Some(style) = style.filter(|_| colored) {
            write!(output, "\x1b[{style}m{value}\x1b[0m")?;
        } else {
            write!(output, "{value}")?;
        }
        if index < row.len() - 1 {
            write!(
                output,
                "{}",
                " ".repeat(widths[index] - value.chars().count() + 2)
            )?;
        }
    }
    writeln!(output)
}
