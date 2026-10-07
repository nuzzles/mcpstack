use crate::results::{OperationOutput, Outcome};
use std::collections::BTreeMap;
use std::fs;
use std::io::{IsTerminal, Write, stderr, stdin};
use std::ops::Range;
use std::path::{Path, PathBuf};

use serde_json::Value as Json;
use toml_edit::{Item, Value};

use super::config::{FileError, Snapshot};
use super::import::prepare;
use super::workflow::{SecretResolver, server_results, warn_runtime_bindings};
use crate::error::AppError;
use crate::schema::Stack;

use super::{Error, default_config};

pub(super) fn run(
    file: PathBuf,
    config: Option<PathBuf>,
    output: &mut OperationOutput<impl Write>,
    non_interactive: bool,
    colored: bool,
) -> Result<(), AppError> {
    let path = config.or_else(default_config).ok_or(Error::ConfigPath)?;
    output.report.config_path = Some(path.to_string_lossy().into_owned());
    let snapshot = Snapshot::read(&path)?;
    let document = fs::read_to_string(file).map_err(AppError::StackRead)?;
    let stack = Stack::from_yaml(&document)?;
    let mut resolver = SecretResolver {
        dry_run: true,
        interactive: !non_interactive && stdin().is_terminal() && stderr().is_terminal(),
        values: BTreeMap::new(),
        cancelled: false,
    };
    let prepared = match &stack {
        Stack::V1(stack) => {
            output.report.stack_schema_version = Some(stack.schema_version);
            prepare(stack, |name| resolver.resolve(name))
        }
    };
    if resolver.cancelled {
        return Err(AppError::ImportApprovalCancelled);
    }
    let definitions = prepared?;
    let changes = snapshot.preview(&definitions)?;
    output.report.servers = server_results(&definitions, &changes, Outcome::Proposed);
    if changes.is_empty() {
        output.report.diff = Some(String::new());
        writeln!(
            output,
            "No changes; existing identical entries were unchanged."
        )?;
        return Ok(());
    }
    let incoming = changes
        .iter()
        .map(|(name, (_, value))| (name.clone(), value.clone()))
        .collect();
    warn_runtime_bindings(&incoming);
    let secrets: Vec<_> = resolver.values.into_values().flatten().collect();
    show_preview(&snapshot, &path, &incoming, &secrets, output, colored)
}

pub(super) fn show_preview(
    snapshot: &Snapshot,
    path: &Path,
    definitions: &BTreeMap<String, toml::Table>,
    secrets: &[String],
    output: &mut OperationOutput<impl Write>,
    colored: bool,
) -> Result<(), AppError> {
    if output.json {
        let mut preview = Vec::new();
        show_diff(snapshot, path, definitions, secrets, &mut preview, false)?;
        output.report.diff = Some(String::from_utf8(preview).map_err(std::io::Error::other)?);
        Ok(())
    } else {
        show_diff(snapshot, path, definitions, secrets, output, colored)
    }
}

pub(super) fn show_diff(
    snapshot: &Snapshot,
    path: &Path,
    definitions: &BTreeMap<String, toml::Table>,
    secrets: &[String],
    output: &mut impl Write,
    colored: bool,
) -> Result<(), AppError> {
    let changes = snapshot.preview(definitions)?;
    let mut before_masks = BTreeMap::new();
    let mut after_masks = BTreeMap::new();
    for (name, (before, after)) in &changes {
        let after_mask = masked_server(name, after, before.as_ref(), secrets)?;
        if let Some(before) = before {
            let mut before_mask = masked_server(name, before, None, secrets)?;
            redact_matching_fields(&mut before_mask, &after_mask);
            before_masks.insert(name.clone(), before_mask);
        }
        after_masks.insert(name.clone(), after_mask);
    }
    let original = snapshot.original_text()?;
    let (_, proposed) = snapshot.proposal(definitions)?;
    let before = redact_document(original, &before_masks)?;
    let after = redact_document(&proposed, &after_masks)?;
    let label = path.to_string_lossy();
    let patch = similar::TextDiff::from_lines(&before, &after)
        .unified_diff()
        .context_radius(2)
        .header(
            if original.is_empty() {
                "/dev/null"
            } else {
                &label
            },
            &label,
        )
        .to_string();
    write_diff(output, &patch, colored)?;
    Ok(())
}

/// A masked incoming field identifies its existing value as private too, even
/// when that value differs and its field name does not identify a credential.
fn redact_matching_fields(before: &mut Json, after: &Json) {
    if matches!(after.as_str(), Some("<redacted>" | "<redacted: changed>")) {
        *before = "<redacted>".into();
        return;
    }
    match (before, after) {
        (Json::Object(before), Json::Object(after)) => {
            for (key, value) in before {
                if let Some(after) = after.get(key) {
                    redact_matching_fields(value, after);
                }
            }
        }
        (Json::Array(before), Json::Array(after)) => {
            for (before, after) in before.iter_mut().zip(after) {
                redact_matching_fields(before, after);
            }
        }
        _ => {}
    }
}

fn masked_server(
    name: &str,
    definition: &toml::Table,
    existing: Option<&toml::Table>,
    secrets: &[String],
) -> Result<Json, AppError> {
    let mut masked = super::export::mask_for_preview(name, definition)?;
    redact_references(&mut masked);
    redact_resolved(&mut masked, secrets);
    let source = serde_json::to_value(definition).map_err(|_| FileError::Config)?;
    let previous = existing
        .map(serde_json::to_value)
        .transpose()
        .map_err(|_| FileError::Config)?;
    annotate_preview(&mut masked, &source, previous.as_ref());
    Ok(masked)
}

/// Redact source tokens in place, preserving every physical newline. Unrelated
/// lines and comments are omitted, so context cannot reveal private settings.
fn redact_document(source: &str, masks: &BTreeMap<String, Json>) -> Result<String, FileError> {
    let document = toml_edit::Document::parse(source).map_err(|_| FileError::Config)?;
    let mask = serde_json::json!({"mcp_servers": masks});
    let mut tokens = Vec::new();
    collect_item(document.as_item(), Some(&mask), false, 0, &mut tokens);
    tokens.sort_by_key(|(span, _, _)| span.start);
    let mut visible = vec![false; source.lines().count()];
    let mut redacted = String::new();
    let mut cursor = 0;
    for (span, replacement, relevant) in tokens {
        if span.start < cursor {
            continue;
        }
        strip_comments(&source[cursor..span.start], &mut redacted);
        let text = &source[span.clone()];
        if relevant {
            let start = source[..span.start].bytes().filter(|b| *b == b'\n').count();
            let end = start + text.bytes().filter(|b| *b == b'\n').count();
            for line in start..=end {
                if let Some(visible) = visible.get_mut(line) {
                    *visible = true;
                }
            }
        }
        if let Some(replacement) = replacement {
            redacted.push_str(&replacement);
            // A masked multiline value must not shift subsequent hunk positions.
            redacted.extend(std::iter::repeat_n(
                '\n',
                text.bytes().filter(|b| *b == b'\n').count(),
            ));
        } else {
            redacted.push_str(text);
        }
        cursor = span.end;
    }
    strip_comments(&source[cursor..], &mut redacted);
    let mut result = String::new();
    for (index, line) in redacted.split_inclusive('\n').enumerate() {
        if visible.get(index) == Some(&true) || line.trim().is_empty() {
            result.push_str(line);
        } else {
            result.push_str("# <unrelated configuration omitted>");
            if line.ends_with('\n') {
                result.push('\n');
            }
        }
    }
    Ok(result)
}

type Token = (Range<usize>, Option<String>, bool);

fn collect_item(
    item: &Item,
    mask: Option<&Json>,
    relevant: bool,
    depth: usize,
    tokens: &mut Vec<Token>,
) {
    match item {
        Item::Table(table) => {
            for (name, item) in table.iter() {
                let child = mask.and_then(|mask| mask.get(name));
                // The first two levels select mcp_servers and a changed server.
                let selected = relevant || (depth == 1 && child.is_some());
                if let Some(span) = table.key(name).and_then(|key| key.span()) {
                    tokens.push((span, None, selected));
                }
                collect_item(item, child, selected, depth + 1, tokens);
            }
        }
        Item::Value(value) => collect_value(value, mask, relevant, depth, tokens),
        Item::ArrayOfTables(tables) => {
            for (index, table) in tables.iter().enumerate() {
                collect_item(
                    &Item::Table(table.clone()),
                    mask.and_then(|mask| mask.get(index)),
                    relevant,
                    depth + 1,
                    tokens,
                );
            }
        }
        Item::None => {}
    }
}

fn collect_value(
    value: &Value,
    mask: Option<&Json>,
    relevant: bool,
    depth: usize,
    tokens: &mut Vec<Token>,
) {
    match value {
        Value::Array(array) => {
            for (index, value) in array.iter().enumerate() {
                collect_value(
                    value,
                    mask.and_then(|mask| mask.get(index)),
                    relevant,
                    depth + 1,
                    tokens,
                );
            }
        }
        Value::InlineTable(table) => {
            for (name, value) in table.iter() {
                let child = mask.and_then(|mask| mask.get(name));
                let selected = relevant || (depth == 1 && child.is_some());
                if let Some(span) = table.key(name).and_then(|key| key.span()) {
                    tokens.push((span, None, selected));
                }
                collect_value(value, child, selected, depth + 1, tokens);
            }
        }
        _ => {
            if let Some(span) = value.span() {
                let replacement = match mask {
                    None => Some("\"<omitted>\"".to_owned()),
                    Some(value)
                        if matches!(value.as_str(), Some("<redacted>" | "<redacted: changed>")) =>
                    {
                        Some(format!("\"{}\"", value.as_str().unwrap()))
                    }
                    Some(_) => None,
                };
                tokens.push((span, replacement, relevant));
            }
        }
    }
}

// Quoted keys and scalar values are already protected by parser token spans.
fn strip_comments(gap: &str, output: &mut String) {
    for line in gap.split_inclusive('\n') {
        if let Some((before, _)) = line.split_once('#') {
            output.push_str(before);
            if line.ends_with('\n') {
                output.push('\n');
            }
        } else {
            output.push_str(line);
        }
    }
}
fn write_diff(output: &mut impl Write, diff: &str, colored: bool) -> std::io::Result<()> {
    if !colored {
        return write!(output, "{diff}");
    }
    for line in diff.lines() {
        let color =
            if line.starts_with("--- ") || line.starts_with("+++ ") || line.starts_with("@@ ") {
                Some(36) // cyan headers
            } else if line.starts_with('+') {
                Some(32) // green additions
            } else if line.starts_with('-') {
                Some(31) // red removals
            } else {
                None
            };
        if let Some(color) = color {
            writeln!(output, "\x1b[{color}m{line}\x1b[0m")?;
        } else {
            writeln!(output, "{line}")?;
        }
    }
    Ok(())
}

/// Mark known hidden value changes without exposing the value, length, or hash.
fn annotate_preview(
    masked: &mut serde_json::Value,
    source: &serde_json::Value,
    previous: Option<&serde_json::Value>,
) {
    if masked.as_str() == Some("<redacted>") {
        if previous.is_some_and(|previous| previous != source) {
            *masked = "<redacted: changed>".into();
        }
    } else {
        match masked {
            serde_json::Value::Object(values) => {
                for (key, value) in values {
                    if let Some(source) = source.get(key) {
                        annotate_preview(value, source, previous.and_then(|value| value.get(key)));
                    }
                }
            }
            serde_json::Value::Array(values) => {
                for (index, value) in values.iter_mut().enumerate() {
                    if let Some(source) = source.get(index) {
                        annotate_preview(
                            value,
                            source,
                            previous.and_then(|value| value.get(index)),
                        );
                    }
                }
            }
            _ => {}
        }
    }
}

fn redact_resolved(value: &mut serde_json::Value, secrets: &[String]) {
    match value {
        serde_json::Value::String(text) => {
            if secrets
                .iter()
                .any(|secret| !secret.is_empty() && text.contains(secret))
            {
                *text = "<redacted>".into();
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                redact_resolved(value, secrets);
            }
        }
        serde_json::Value::Object(values) => {
            for value in values.values_mut() {
                redact_resolved(value, secrets);
            }
        }
        _ => {}
    }
}

fn redact_references(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(values) if values.contains_key("$env") => {
            *value = serde_json::Value::String("<redacted>".into());
        }
        serde_json::Value::Object(values) => {
            for value in values.values_mut() {
                redact_references(value);
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                redact_references(value);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redaction_preserves_lines_for_multiline_inline_and_dotted_values() {
        for source in [
            "model='private-model' # private-comment\n[mcp_servers.\"a.b#c\"]\ncommand='tool#hash'\nargs=[\n  '--token', # private-comment\n  '''first-secret\nsecond-secret''',\n]\n[mcp_servers.\"a.b#c\".env]\nAPI_KEY='env-secret'\n",
            "model='private-model'\nmcp_servers = {\"a.b#c\" = {command='tool#hash', env={API_KEY='env-secret'}}, unrelated={command='private-command'}}\n",
            "mcp_servers.\"a.b#c\".command='tool#hash'\nmcp_servers.\"a.b#c\".env.API_KEY='env-secret'\n",
        ] {
            let parsed: toml::Table = toml::from_str(source).unwrap();
            let server = parsed["mcp_servers"]["a.b#c"].as_table().unwrap();
            let mask = masked_server("a.b#c", server, None, &[]).unwrap();
            let redacted =
                redact_document(source, &BTreeMap::from([("a.b#c".into(), mask)])).unwrap();
            assert_eq!(redacted.lines().count(), source.lines().count());
            assert!(redacted.contains("tool#hash"), "{redacted}");
            assert!(redacted.contains("<redacted>"), "{redacted}");
            for secret in [
                "private-model",
                "private-comment",
                "first-secret",
                "second-secret",
                "env-secret",
                "private-command",
            ] {
                assert!(!redacted.contains(secret), "{secret} leaked: {redacted}");
            }
        }
    }
}
