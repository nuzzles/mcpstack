use std::collections::{BTreeMap, BTreeSet};
use std::io::{IsTerminal, Write, stderr, stdin};
use std::path::{Path, PathBuf};
use std::{env, fs};

use clap::{Args, Subcommand};
use dialoguer::{Confirm, Input, Password, Select};

use crate::error::AppError;
use crate::importers::codex::prepare;
use crate::importers::codex_fs::Snapshot;
use crate::schema::Stack;

/// Preview and approve server additions and replacements.
#[derive(Args)]
pub struct Import {
    /// Preview and approve changes without writing files; prompts still apply.
    #[arg(long, global = true)]
    dry_run: bool,
    /// Approve all server additions and replacements without prompting.
    #[arg(short = 'y', long, global = true)]
    auto_approve: bool,
    #[command(subcommand)]
    client: Client,
}

#[derive(Subcommand)]
enum Client {
    Codex {
        /// Stack file to import; secret references resolve from the environment.
        #[arg(value_name = "FILE")]
        file: PathBuf,
        /// Write this Codex TOML file instead of the default configuration.
        #[arg(long, value_name = "PATH")]
        config: Option<PathBuf>,
    },
}

impl Import {
    pub fn run(
        self,
        output: &mut impl Write,
        non_interactive: bool,
        colored: bool,
    ) -> Result<(), AppError> {
        if self.dry_run {
            tracing::warn!("Dry run; no changes will be committed.");
        }
        match self.client {
            Client::Codex { file, config } => {
                let path = config
                    .or_else(super::export::default_config)
                    .ok_or(AppError::ConfigPath)?;
                let snapshot = Snapshot::read(&path)?;
                let interactive =
                    !non_interactive && stdin().is_terminal() && stderr().is_terminal();
                if !self.dry_run {
                    if !self.auto_approve && !interactive {
                        return Err(AppError::ImportApprovalRequired);
                    }
                    let default = snapshot.default_backup_path()?;
                    let create_backup = self.auto_approve
                        || Confirm::new()
                            .with_prompt("Create a backup before importing?")
                            .default(true)
                            .report(false)
                            .interact()
                            .map_err(|_| AppError::ImportApprovalCancelled)?;
                    if create_backup {
                        let backup_path = if self.auto_approve {
                            default
                        } else {
                            let default_text = default.to_string_lossy().into_owned();
                            let selected: String = Input::new()
                                .with_prompt("Backup path")
                                .default(default_text.clone())
                                .report(false)
                                .interact_text()
                                .map_err(|_| AppError::ImportApprovalCancelled)?;
                            if selected == default_text {
                                default
                            } else {
                                PathBuf::from(selected)
                            }
                        };
                        snapshot.create_backup_at(&backup_path)?;
                    }
                }
                let document = fs::read_to_string(file).map_err(AppError::StackRead)?;
                let stack = Stack::from_yaml(&document)?;
                let mut resolver = SecretResolver {
                    dry_run: self.dry_run,
                    interactive: (self.dry_run || !self.auto_approve) && interactive,
                    values: BTreeMap::new(),
                    cancelled: false,
                };
                let prepared = match &stack {
                    Stack::V1(stack) => prepare(stack, |name| resolver.resolve(name)),
                };
                if resolver.cancelled {
                    return Err(AppError::ImportApprovalCancelled);
                }
                let definitions = prepared?;
                let changes = snapshot.preview(&definitions)?;
                let additions: BTreeMap<_, _> = changes
                    .iter()
                    .map(|(name, (_, definition))| (name.clone(), definition.clone()))
                    .collect();
                warn_runtime_bindings(&additions);
                let secrets: Vec<_> = resolver.values.values().flatten().cloned().collect();
                let diffs: BTreeMap<_, _> = additions
                    .iter()
                    .map(|(name, definition)| {
                        let diff = server_diff(
                            &path,
                            name,
                            changes[name].0.as_ref(),
                            definition,
                            &secrets,
                        )?;
                        Ok((name.clone(), diff))
                    })
                    .collect::<Result<_, AppError>>()?;
                if additions.is_empty() {
                    writeln!(
                        output,
                        "No changes; existing identical entries were unchanged."
                    )?;
                    return Ok(());
                }
                if !self.auto_approve && !interactive {
                    return Err(AppError::ImportApprovalRequired);
                }
                // Show the whole proposal before even a bulk approval choice.
                for diff in diffs.values() {
                    write_diff(output, diff, colored)?;
                }
                output.flush()?;
                let approved = if self.auto_approve {
                    additions
                } else {
                    select_servers(additions, |name, current, total| {
                        let replacing = changes[name].0.is_some();
                        let action = if replacing {
                            "Replace existing server"
                        } else {
                            "Import"
                        };
                        Select::new()
                            .with_prompt(format!("Server {current}/{total}: {action} {name:?}?"))
                            .items([
                                "No, skip this server",
                                if replacing {
                                    "Yes, replace this server"
                                } else {
                                    "Yes, import this server"
                                },
                                "No, skip this and all remaining servers",
                                if replacing {
                                    "Yes, replace this server and approve all remaining changes"
                                } else {
                                    "Yes, import this and all remaining servers (including replacements)"
                                },
                            ])
                            .default(0)
                            .report(false)
                            .interact_opt()
                            .map_err(|_| AppError::ImportApprovalCancelled)?
                            .ok_or(AppError::ImportApprovalCancelled)
                    })?
                };
                if approved.is_empty() {
                    writeln!(
                        output,
                        "No servers approved; client configuration was not changed."
                    )?;
                    return Ok(());
                }
                if self.dry_run {
                    writeln!(output, "Would import {} server(s).", approved.len())?;
                    return Ok(());
                }
                // Check that the target still matches the snapshot before
                // atomic replacement.
                let added = snapshot.apply(&approved)?;
                writeln!(
                    output,
                    "Imported {added} server(s); existing identical entries were unchanged."
                )?;
                Ok(())
            }
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

/// Cache each reference so repeated uses ask only once. All comparisons use
/// real values; dry-run never proceeds to backup or file writes.
struct SecretResolver {
    dry_run: bool,
    interactive: bool,
    values: BTreeMap<String, Option<String>>,
    cancelled: bool,
}

impl SecretResolver {
    fn resolve(&mut self, name: &str) -> Option<String> {
        if self.cancelled {
            return None;
        }
        if let Some(value) = self.values.get(name) {
            return value.clone();
        }
        let mut value = env::var(name)
            .ok()
            .filter(|value| !value.is_empty() && !value.contains('\0'));
        if value.is_none() {
            if self.interactive {
                match Password::new()
                    .with_prompt(if self.dry_run {
                        format!("Enter secret for {name}")
                    } else {
                        format!("Enter secret for {name} (written as a literal in config)")
                    })
                    .report(false)
                    .validate_with(|value: &String| {
                        if value.contains('\0') {
                            Err("Value must not contain NUL")
                        } else {
                            Ok(())
                        }
                    })
                    .interact()
                {
                    Ok(secret) => value = Some(secret),
                    Err(_) => self.cancelled = true,
                }
            } else {
                tracing::warn!(
                    "Environment variable {name} is unset or unusable; this masked field needs a value before import."
                );
            }
        }
        self.values.insert(name.into(), value.clone());
        value
    }
}

fn warn_runtime_bindings(additions: &BTreeMap<String, toml::Table>) {
    let mut names = BTreeSet::new();
    for definition in additions.values() {
        if let Some(name) = definition
            .get("bearer_token_env_var")
            .and_then(toml::Value::as_str)
        {
            names.insert(name);
        }
        if let Some(headers) = definition
            .get("env_http_headers")
            .and_then(toml::Value::as_table)
        {
            names.extend(headers.values().filter_map(toml::Value::as_str));
        }
        if let Some(variables) = definition.get("env_vars").and_then(toml::Value::as_array) {
            names.extend(
                variables
                    .iter()
                    .filter(|value| {
                        value.get("source").and_then(toml::Value::as_str) != Some("remote")
                    })
                    .filter_map(|value| {
                        value
                            .as_str()
                            .or_else(|| value.get("name").and_then(toml::Value::as_str))
                    }),
            );
        }
    }
    for name in names {
        if env::var(name)
            .ok()
            .is_none_or(|value| value.is_empty() || value.contains('\0'))
        {
            tracing::warn!(
                "Runtime environment variable {name} is unset or unusable in this shell. Import may not work as intended until it is available to Codex; its binding will be preserved."
            );
        }
    }
}

fn select_servers(
    additions: BTreeMap<String, toml::Table>,
    mut choose: impl FnMut(&str, usize, usize) -> Result<usize, AppError>,
) -> Result<BTreeMap<String, toml::Table>, AppError> {
    let total = additions.len();
    let mut remaining = None;
    let mut approved = BTreeMap::new();
    for (index, (name, definition)) in additions.into_iter().enumerate() {
        let import = if let Some(choice) = remaining {
            choice
        } else {
            let choice = choose(&name, index + 1, total)?;
            if choice >= 2 {
                remaining = Some(choice == 3);
            }
            choice == 1 || choice == 3
        };
        if import {
            approved.insert(name, definition);
        }
    }
    Ok(approved)
}

/// Section-level unified preview; include no unrelated private config.
fn server_diff(
    path: &Path,
    name: &str,
    existing: Option<&toml::Table>,
    definition: &toml::Table,
    secrets: &[String],
) -> Result<String, AppError> {
    let before = existing
        .map(|table| render_server(name, table, None, secrets))
        .transpose()?
        .unwrap_or_default();
    let after = render_server(name, definition, existing, secrets)?;
    let label = format!("{} (mcp_servers.{name})", path.display());
    let old_label = if existing.is_some() {
        &label
    } else {
        "/dev/null"
    };
    Ok(similar::TextDiff::from_lines(&before, &after)
        .unified_diff()
        .context_radius(2)
        .header(old_label, &label)
        .to_string())
}

fn render_server(
    name: &str,
    definition: &toml::Table,
    existing: Option<&toml::Table>,
    secrets: &[String],
) -> Result<String, AppError> {
    let mut config = crate::exporters::codex::mask_for_preview(name, definition)?;
    redact_references(&mut config);
    redact_resolved(&mut config, secrets);
    let source = serde_json::to_value(definition)
        .map_err(|_| crate::importers::codex::ImportError::Definition)?;
    let previous = existing
        .map(serde_json::to_value)
        .transpose()
        .map_err(|_| crate::importers::codex::ImportError::Definition)?;
    annotate_preview(&mut config, &source, previous.as_ref());
    toml::to_string(&serde_json::json!({"mcp_servers": {name: config}}))
        .map_err(|_| crate::importers::codex::ImportError::Definition.into())
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

    fn additions() -> BTreeMap<String, toml::Table> {
        ["a", "b", "c"]
            .map(|name| (name.into(), toml::Table::new()))
            .into()
    }

    #[test]
    fn approvals_are_per_server_and_bulk_choices_apply_to_remaining() {
        let mut calls = Vec::new();
        let selected = select_servers(additions(), |name, current, total| {
            calls.push((name.to_string(), current, total));
            Ok(if name == "b" { 1 } else { 0 })
        })
        .unwrap();
        assert_eq!(selected.keys().collect::<Vec<_>>(), [&"b".to_string()]);
        assert_eq!(
            calls,
            [("a".into(), 1, 3), ("b".into(), 2, 3), ("c".into(), 3, 3)]
        );
        for (choice, expected) in [(2, 0), (3, 3)] {
            let mut calls = 0;
            let selected = select_servers(additions(), |_, _, _| {
                calls += 1;
                Ok(choice)
            })
            .unwrap();
            assert_eq!(calls, 1);
            assert_eq!(selected.len(), expected);
        }
    }

    #[test]
    fn cancelling_after_approval_aborts_the_entire_selection() {
        assert!(matches!(
            select_servers(additions(), |name, _, _| {
                if name == "a" {
                    Ok(1)
                } else {
                    Err(AppError::ImportApprovalCancelled)
                }
            }),
            Err(AppError::ImportApprovalCancelled)
        ));
    }
}
