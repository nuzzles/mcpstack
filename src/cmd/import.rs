use std::collections::{BTreeMap, BTreeSet};
use std::io::{IsTerminal, Write, stderr, stdin};
use std::path::PathBuf;
use std::{env, fs};

use clap::{Args, Subcommand};
use dialoguer::{Confirm, Input, Password, Select};

use crate::error::AppError;
use crate::importers::codex::prepare;
use crate::importers::codex_fs::Snapshot;
use crate::schema::Stack;

/// Approve and apply server additions and replacements.
#[derive(Args)]
pub struct Import {
    /// Run import prompts, then show the diff for approved changes without writing files.
    #[arg(long, global = true)]
    dry_run: bool,
    /// Approve all server additions and replacements without prompting.
    #[arg(short = 'y', long, global = true)]
    auto_approve: bool,
    #[command(subcommand)]
    client: Client,
}

#[derive(Subcommand)]
pub(super) enum Client {
    Codex {
        /// Stack file to compare or import.
        #[arg(value_name = "FILE")]
        file: PathBuf,
        /// Use this Codex TOML file instead of the default configuration.
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
                    snapshot.ensure_write_supported()?;
                    let backup_path = choose_backup_path(
                        &snapshot,
                        self.auto_approve,
                        || {
                            Confirm::new()
                                .with_prompt("Create a backup before importing?")
                                .default(true)
                                .report(false)
                                .interact()
                                .map_err(|_| AppError::ImportApprovalCancelled)
                        },
                        |default| {
                            let mut input = Input::<String>::new()
                                .with_prompt("Backup path")
                                .report(false);
                            let default_text =
                                default.map(|path| path.to_string_lossy().into_owned());
                            if let Some(text) = &default_text {
                                input = input.default(text.clone());
                            } else {
                                tracing::warn!(
                                    "Cannot determine a default backup path; enter an unused custom path."
                                );
                            }
                            let selected = input
                                .interact_text()
                                .map_err(|_| AppError::ImportApprovalCancelled)?;
                            if default_text.as_ref() == Some(&selected) {
                                Ok(default.unwrap().to_path_buf())
                            } else {
                                Ok(PathBuf::from(selected))
                            }
                        },
                    )?;
                    if let Some(backup_path) = backup_path {
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
                    let secrets: Vec<_> = resolver.values.into_values().flatten().collect();
                    super::diff::show_diff(&snapshot, &path, &approved, &secrets, output, colored)?;
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

/// Discover numbered backups only after the user requests one. Interactive
/// callers can still choose a custom path when default discovery fails.
fn choose_backup_path(
    snapshot: &Snapshot,
    auto_approve: bool,
    confirm: impl FnOnce() -> Result<bool, AppError>,
    choose: impl FnOnce(Option<&std::path::Path>) -> Result<PathBuf, AppError>,
) -> Result<Option<PathBuf>, AppError> {
    if auto_approve {
        return Ok(Some(snapshot.default_backup_path()?));
    }
    if !confirm()? {
        return Ok(None);
    }
    let default = snapshot.default_backup_path().ok();
    choose(default.as_deref()).map(Some)
}

/// Cache each reference so repeated uses ask only once. All comparisons use
/// real values; dry-run never proceeds to backup or file writes.
pub(super) struct SecretResolver {
    pub(super) dry_run: bool,
    pub(super) interactive: bool,
    pub(super) values: BTreeMap<String, Option<String>>,
    pub(super) cancelled: bool,
}

impl SecretResolver {
    pub(super) fn resolve(&mut self, name: &str) -> Option<String> {
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

pub(super) fn warn_runtime_bindings(additions: &BTreeMap<String, toml::Table>) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unavailable_default_backup_can_be_skipped_or_replaced_with_custom_path() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        // An unrepresentable generation makes numbered discovery fail on every
        // platform, just as a directory that cannot be listed would.
        fs::write(
            directory.path().join("config.toml.~18446744073709551616~"),
            "existing",
        )
        .unwrap();
        let snapshot = Snapshot::read(&path).unwrap();
        assert!(snapshot.default_backup_path().is_err());
        assert!(
            choose_backup_path(
                &snapshot,
                false,
                || Ok(false),
                |_| { panic!("skipping backup must not ask for a path") }
            )
            .unwrap()
            .is_none()
        );
        let custom = directory.path().join("custom.toml");
        let selected = choose_backup_path(
            &snapshot,
            false,
            || Ok(true),
            |default| {
                assert!(default.is_none());
                Ok(custom.clone())
            },
        )
        .unwrap();
        assert_eq!(selected, Some(custom));
        assert!(
            choose_backup_path(
                &snapshot,
                true,
                || panic!("automatic backup must not prompt"),
                |_| { panic!("automatic backup must not ask for a path") }
            )
            .is_err()
        );
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    }

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
