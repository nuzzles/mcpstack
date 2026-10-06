use std::collections::BTreeMap;
use std::io::{IsTerminal, Write, stderr, stdin};
use std::path::{Path, PathBuf};
use std::{env, fs};

use clap::{Args, Subcommand};
use dialoguer::Select;

use crate::error::AppError;
use crate::importers::codex::prepare;
use crate::importers::codex_fs::Snapshot;
use crate::schema::StackV1;

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
                let stack = StackV1::from_yaml(&document)?;
                let mut secrets = Vec::new();
                let definitions = prepare(&stack, |name| {
                    let value = env::var(name).ok();
                    if let Some(value) = &value {
                        secrets.push(value.clone());
                    }
                    value
                })?;
                let additions = snapshot.additions(&definitions)?;
                let diffs: BTreeMap<_, _> = additions
                    .iter()
                    .map(|(name, definition)| {
                        Ok((
                            name.clone(),
                            server_diff(&path, name, definition, &secrets)?,
                        ))
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

/// Diff only the new server section, so unrelated private config never reaches
/// output. References and recognized literal credentials are redacted.
fn server_diff(
    path: &Path,
    name: &str,
    definition: &toml::Table,
    secrets: &[String],
) -> Result<String, AppError> {
    let native = serde_json::to_value(definition)
        .map_err(|_| crate::importers::codex::ImportError::Definition)?;
    let config = toml::to_string(&serde_json::json!({"mcp_servers": {name: native}}))
        .map_err(|_| crate::importers::codex::ImportError::Definition)?;
    let masked = crate::exporters::codex::export(&config, false)?;
    let mut value = serde_json::to_value(&masked)
        .map_err(|_| crate::importers::codex::ImportError::Definition)?;
    redact_references(&mut value);
    let config = &mut value["servers"][name]["config"];
    redact_resolved(config, secrets);
    let rendered = toml::to_string(&serde_json::json!({"mcp_servers": {name: config}}))
        .map_err(|_| crate::importers::codex::ImportError::Definition)?;
    // Section-level unified diff: imports only add missing servers, never
    // overwrite existing definitions. This is a review preview, not a patch.
    let mut diff = format!(
        "--- /dev/null\n+++ {} (mcp_servers.{name})\n@@ -0,0 +1,{} @@\n",
        path.display(),
        rendered.lines().count()
    );
    for line in rendered.lines() {
        diff.push('+');
        diff.push_str(line);
        diff.push('\n');
    }
    Ok(diff)
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
