use std::collections::{BTreeMap, BTreeSet};
use std::io::{IsTerminal, Write, stderr, stdin};
use std::path::{Path, PathBuf};
use std::{env, fs};

use clap::{Args, Subcommand};
use dialoguer::{Password, Select};

use crate::error::AppError;
use crate::importers::codex::prepare;
use crate::importers::codex_fs::Snapshot;
use crate::schema::Stack;

/// Preview and approve server additions without installing server software.
#[derive(Args)]
pub struct Import {
    /// Print redacted per-server diffs without changing any files or prompting.
    #[arg(long, global = true)]
    dry_run: bool,
    /// Approve all server additions without prompting.
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
    pub fn run(self, output: &mut impl Write, non_interactive: bool) -> Result<(), AppError> {
        match self.client {
            Client::Codex { file, config } => {
                let path = config
                    .or_else(super::export::default_config)
                    .ok_or(AppError::ConfigPath)?;
                let snapshot = Snapshot::read(&path)?;
                let document = fs::read_to_string(file).map_err(AppError::StackRead)?;
                let stack = Stack::from_yaml(&document)?;
                let mut resolver = SecretResolver {
                    dry_run: self.dry_run,
                    interactive: !self.auto_approve
                        && !non_interactive
                        && stdin().is_terminal()
                        && stderr().is_terminal(),
                    values: BTreeMap::new(),
                    unresolved: BTreeSet::new(),
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
                let additions = if self.dry_run {
                    for (name, (existing, _)) in &changes {
                        if existing.is_some() {
                            tracing::warn!(
                                "Server {name} differs from the existing configuration; actual import requires this conflict to be resolved. Unresolved preview values may affect this comparison."
                            );
                        }
                    }
                    changes
                        .iter()
                        .map(|(name, (_, definition))| (name.clone(), definition.clone()))
                        .collect()
                } else {
                    snapshot.additions(&definitions)?
                };
                warn_runtime_bindings(&additions);
                let secrets: Vec<_> = resolver
                    .values
                    .iter()
                    .filter(|(name, _)| !resolver.unresolved.contains(*name))
                    .filter_map(|(_, value)| value.clone())
                    .collect();
                let diffs: BTreeMap<_, _> = additions
                    .iter()
                    .map(|(name, definition)| {
                        let mut diff = server_diff(
                            &path,
                            name,
                            changes[name].0.as_ref(),
                            definition,
                            &secrets,
                        )?;
                        for variable in &resolver.unresolved {
                            diff = diff.replace(
                                &format!("https://unresolved.invalid/{variable}"),
                                &format!("<unresolved: {variable}>"),
                            );
                        }
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
                if !self.dry_run
                    && !self.auto_approve
                    && (non_interactive || !stdin().is_terminal() || !stderr().is_terminal())
                {
                    return Err(AppError::ImportApprovalRequired);
                }
                // Show the whole proposal before even a bulk approval choice.
                for diff in diffs.values() {
                    write!(output, "{diff}")?;
                }
                output.flush()?;
                if self.dry_run {
                    return Ok(());
                }
                let approved = if self.auto_approve {
                    additions
                } else {
                    select_servers(additions, |name, current, total| {
                        Select::new()
                            .with_prompt(format!("Server {current}/{total}: Import {name}?"))
                            .items([
                                "Skip this server",
                                "Import this server",
                                "Skip this and all remaining servers",
                                "Import this and all remaining servers",
                            ])
                            .default(0)
                            .report(false)
                            .interact_opt()
                            .map_err(|_| AppError::ImportApprovalCancelled)?
                            .ok_or(AppError::ImportApprovalCancelled)
                    })?
                };
                if approved.is_empty() {
                    writeln!(output, "No servers approved; no files were changed.")?;
                    return Ok(());
                }
                // All selections complete before backup or writes. Backup uses
                // the reviewed snapshot and fails if the target has changed.
                snapshot.create_backup()?;
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

/// Cache each reference so repeated uses ask only once. Preview sentinels are
/// confined to dry-run: that path returns before backup or file writes.
struct SecretResolver {
    dry_run: bool,
    interactive: bool,
    values: BTreeMap<String, Option<String>>,
    unresolved: BTreeSet<String>,
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
            tracing::warn!(
                "Environment variable {name} is unset or unusable; this masked field needs a value before import."
            );
            if self.dry_run {
                self.unresolved.insert(name.into());
                // A valid URL also passes string-only command/argument/header
                // validation. It is never written or executed.
                value = Some(format!("https://unresolved.invalid/{name}"));
            } else if self.interactive {
                match Password::new()
                    .with_prompt(format!(
                        "Enter secret for {name} (written as a literal in config)"
                    ))
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
        .map(|table| render_server(name, table, secrets))
        .transpose()?
        .unwrap_or_default();
    let after = render_server(name, definition, secrets)?;
    let old_label = if existing.is_some() {
        format!("{} (mcp_servers.{name})", path.display())
    } else {
        "/dev/null".into()
    };
    let old_count = before.lines().count();
    let mut diff = format!(
        "--- {old_label}\n+++ {} (mcp_servers.{name})\n@@ -{},{} +1,{} @@\n",
        path.display(),
        usize::from(old_count != 0),
        old_count,
        after.lines().count()
    );
    for (prefix, text) in [('-', before.as_str()), ('+', after.as_str())] {
        for line in text.lines() {
            diff.push(prefix);
            diff.push_str(line);
            diff.push('\n');
        }
    }
    Ok(diff)
}

fn render_server(
    name: &str,
    definition: &toml::Table,
    secrets: &[String],
) -> Result<String, AppError> {
    let mut config = crate::exporters::codex::mask_for_preview(name, definition)?;
    redact_references(&mut config);
    redact_resolved(&mut config, secrets);
    toml::to_string(&serde_json::json!({"mcp_servers": {name: config}}))
        .map_err(|_| crate::importers::codex::ImportError::Definition.into())
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
