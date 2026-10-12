use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::io::{IsTerminal, Write, stderr, stdin};
use std::path::{Path, PathBuf};
use tokio::fs;

use dialoguer::{Confirm, Input, Select};

use super::config::Snapshot;
use super::export::ExportError;
use super::import::{ImportError, prepare};
use super::{Error, default_config};
use crate::error::AppError;
use crate::exporters::to_yaml;
use crate::integrations::secrets::SecretResolver;
use crate::schema::v1::ApprovalMode;
use crate::schema::{Server, Stack};

pub(super) fn apply_permissions(
    stack: &Stack,
    definitions: &mut BTreeMap<String, toml::Table>,
) -> Result<(), ImportError> {
    let Stack::V1(stack) = stack;
    for (name, server) in &stack.servers {
        let Server::Portable {
            permissions: Some(policy),
            ..
        } = server
        else {
            continue;
        };
        let Some(definition) = definitions.get_mut(name) else {
            continue;
        };
        let fail = |reason| ImportError::Permissions(name.clone(), reason);
        let mut default = policy.default_mode;
        let mut tools = BTreeMap::new();
        let mut global = None;
        if policy
            .auto
            .iter()
            .chain(&policy.writes)
            .any(|tool| tool.contains('*'))
        {
            return Err(fail(
                "auto and writes rules require exact tool names; use default_mode for a fallback",
            ));
        }
        for (rules, mode) in [
            (&policy.allow, ApprovalMode::Allow),
            (&policy.ask, ApprovalMode::Ask),
            (&policy.auto, ApprovalMode::Auto),
            (&policy.writes, ApprovalMode::Writes),
        ] {
            for tool in rules {
                if tool == "*" {
                    if global.replace(mode).is_some_and(|old| old != mode) {
                        return Err(fail("overlapping server-wide decisions"));
                    }
                } else if tool.contains('*') {
                    return Err(fail("tool patterns require an exact tool name"));
                } else if tools
                    .insert(tool.clone(), mode)
                    .is_some_and(|old| old != mode)
                {
                    return Err(fail("multiple approval decisions for one tool"));
                }
            }
        }
        if let Some(mode) = global {
            if default.is_some_and(|fallback| fallback != mode) {
                return Err(fail(
                    "a server-wide rule conflicts with the fallback decision",
                ));
            }
            if mode == ApprovalMode::Ask && tools.values().any(|mode| *mode != ApprovalMode::Ask) {
                return Err(fail(
                    "server-wide ask takes precedence over tool approval rules",
                ));
            }
            if !policy.auto.is_empty() || !policy.writes.is_empty() {
                return Err(fail(
                    "server-wide rules cannot preserve automatic per-tool decisions",
                ));
            }
            default = Some(mode);
        }
        if policy.deny.iter().any(|tool| tool.contains('*')) {
            return Err(fail("deny patterns require an exact tool name"));
        }
        if let Some(mode) = default {
            definition.insert(
                "default_tools_approval_mode".into(),
                toml::Value::String(mode_text(mode).into()),
            );
        }
        if !tools.is_empty() {
            definition.insert(
                "tools".into(),
                toml::Value::Table(
                    tools
                        .into_iter()
                        .map(|(tool, mode)| {
                            (
                                tool,
                                toml::Value::Table(toml::Table::from_iter([(
                                    "approval_mode".into(),
                                    toml::Value::String(mode_text(mode).into()),
                                )])),
                            )
                        })
                        .collect(),
                ),
            );
        }
        if !policy.deny.is_empty() {
            let mut denied = definition
                .get("disabled_tools")
                .and_then(toml::Value::as_array)
                .cloned()
                .unwrap_or_default();
            for tool in &policy.deny {
                let value = toml::Value::String(tool.clone());
                if !denied.contains(&value) {
                    denied.push(value);
                }
            }
            definition.insert("disabled_tools".into(), toml::Value::Array(denied));
        }
    }
    Ok(())
}

pub(super) type Preliminary = (BTreeMap<String, toml::Table>, Vec<String>, bool);

pub(super) fn preliminary_definitions(
    snapshot: &Snapshot,
    stack: &Stack,
    include_permissions: bool,
) -> Result<Preliminary, AppError> {
    let mut resolver = SecretResolver {
        dry_run: true,
        interactive: false,
        preview_missing: true,
        missing_used: false,
        values: BTreeMap::new(),
        cancelled: false,
    };
    let mut definitions = match stack {
        Stack::V1(stack) => prepare(stack, |name| resolver.resolve(name))?,
    };
    retain_target_policy(snapshot, stack, &mut definitions, include_permissions)?;
    if include_permissions {
        apply_permissions(stack, &mut definitions)?;
    }
    let secrets = resolver.values.into_values().flatten().collect();
    Ok((definitions, secrets, resolver.missing_used))
}

/// Fields excluded from portable stacks remain owned by the destination Codex config.
/// Permission fields are copied only when the operation did not request a policy restore.
pub(super) fn retain_target_policy(
    snapshot: &Snapshot,
    stack: &Stack,
    definitions: &mut BTreeMap<String, toml::Table>,
    include_permissions: bool,
) -> Result<(), super::config::FileError> {
    let existing = snapshot.servers()?;
    let Stack::V1(stack) = stack;
    for (name, definition) in definitions {
        let Some(current) = existing.get(name) else {
            continue;
        };
        for field in ["sandbox_mode", "approval_policy"] {
            if let Some(value) = current.get(field) {
                definition.entry(field).or_insert_with(|| value.clone());
            }
        }
        let restoring_permissions = include_permissions
            && matches!(
                stack.servers.get(name),
                Some(Server::Portable {
                    permissions: Some(_),
                    ..
                })
            );
        if !restoring_permissions {
            for field in ["default_tools_approval_mode", "tools", "disabled_tools"] {
                if let Some(value) = current.get(field) {
                    definition.entry(field).or_insert_with(|| value.clone());
                }
            }
        }
    }
    Ok(())
}

fn mode_text(mode: ApprovalMode) -> &'static str {
    match mode {
        ApprovalMode::Auto => "auto",
        ApprovalMode::Ask => "prompt",
        ApprovalMode::Writes => "writes",
        ApprovalMode::Allow => "approve",
    }
}

pub(crate) async fn run_list(
    config: Option<PathBuf>,
    filter: crate::integrations::reserved::ServerFilter,
    output: &mut impl Write,
    colored: bool,
) -> Result<(), AppError> {
    let path = config.or_else(default_config).ok_or(Error::ConfigPath)?;
    let document = fs::read_to_string(path)
        .await
        .map_err(AppError::ConfigRead)?;
    let config: toml::Value = toml::from_str(&document).map_err(|_| AppError::ListConfig)?;
    let config = serde_json::to_value(config).map_err(|_| AppError::ListConfig)?;
    crate::integrations::listing::write(
        output,
        &config,
        crate::integrations::listing::Source::Codex,
        filter,
        colored,
    )
}

pub(crate) async fn run_export(
    config: Option<PathBuf>,
    filter: crate::integrations::reserved::ServerFilter,
    include_permissions: bool,
    output: &mut impl Write,
    non_interactive: bool,
    expose_secrets: bool,
) -> Result<(), AppError> {
    tracing::debug!("Exporting Codex MCP configuration");
    let path = config.or_else(default_config).ok_or(Error::ConfigPath)?;
    let document = fs::read_to_string(path)
        .await
        .map_err(AppError::ConfigRead)?;
    let stack = if expose_secrets {
        super::export::export_filtered(&document, true, filter)?
    } else if non_interactive || !stdin().is_terminal() || !stderr().is_terminal() {
        super::export::export_filtered(&document, false, filter)?
    } else {
        let mut remaining_choice = None;
        super::export::export_with_filter(&document, filter, |path, current, total| {
            if let Some(choice) = remaining_choice {
                return Ok(choice);
            }
            let selected = Select::new()
                .with_prompt(format!(
                    "Secret {current}/{total}: Include {path} as a literal?"
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
                .map_err(|_| ExportError::Prompt)?
                .ok_or(ExportError::Prompt)?;
            if selected >= 2 {
                remaining_choice = Some(selected == 3);
            }
            Ok(selected == 1 || selected == 3)
        })?
    };
    // Prepare the complete result before exposing any content on stdout.
    let stack = crate::integrations::portable::normalize(
        stack,
        crate::integrations::portable::Source::Codex,
        include_permissions,
    )
    .map_err(ExportError::Portable)?;
    let yaml = to_yaml(&stack.into()).map_err(|_| ExportError::Stack)?;
    write!(output, "{yaml}")?;
    Ok(())
}

#[expect(
    clippy::too_many_arguments,
    reason = "CLI workflow accepts separate command flags"
)]
pub(crate) async fn run_import(
    file: PathBuf,
    config: Option<PathBuf>,
    selection: crate::integrations::selection::Selection,
    include_permissions: bool,
    output: &mut impl Write,
    non_interactive: bool,
    colored: bool,
    dry_run: bool,
    auto_approve: bool,
) -> Result<(), AppError> {
    if dry_run {
        tracing::warn!("Dry run; no changes will be committed.");
    }
    let path = config.or_else(default_config).ok_or(Error::ConfigPath)?;
    let snapshot = Snapshot::read(&path).await?;
    // Explicit selections must be checked before prompts or backup creation.
    let selected_stack = if selection.is_empty() {
        None
    } else {
        Some(selection.read_stack(&file).await?)
    };
    let interactive = !non_interactive && stdin().is_terminal() && stderr().is_terminal();
    if !auto_approve && !interactive {
        return Err(AppError::ImportApprovalRequired);
    }
    let stack: Stack = match selected_stack {
        Some(stack) => stack,
        None => selection.read_stack(&file).await?,
    }
    .into();
    let (preliminary, _, missing) =
        preliminary_definitions(&snapshot, &stack, include_permissions)?;
    if missing {
        let changes = snapshot.preview(&preliminary)?;
        if !changes.is_empty() {
            writeln!(
                output,
                "Preliminary changes; missing secrets will be requested for an exact comparison."
            )?;
            let added = changes
                .iter()
                .filter(|(_, (before, _))| before.is_none())
                .map(|(name, _)| name.clone())
                .collect();
            let modified = changes
                .iter()
                .filter(|(_, (before, _))| before.is_some())
                .map(|(name, _)| name.clone())
                .collect();
            super::super::write_change_summary(
                output,
                &path,
                &added,
                &modified,
                &BTreeSet::new(),
                colored,
            )?;
        }
    }
    let mut resolver = SecretResolver {
        dry_run,
        interactive: (dry_run || !auto_approve) && interactive,
        preview_missing: false,
        missing_used: false,
        values: BTreeMap::new(),
        cancelled: false,
    };
    let prepared = match &stack {
        Stack::V1(stack) => prepare(stack, |name| resolver.resolve(name)),
    };
    if resolver.cancelled {
        return Err(AppError::ImportApprovalCancelled);
    }
    let mut definitions = prepared?;
    retain_target_policy(&snapshot, &stack, &mut definitions, include_permissions)?;
    if include_permissions {
        apply_permissions(&stack, &mut definitions)?;
    }
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
    let added = changes
        .iter()
        .filter(|(_, (before, _))| before.is_none())
        .map(|(name, _)| name.clone())
        .collect();
    let modified = changes
        .iter()
        .filter(|(_, (before, _))| before.is_some())
        .map(|(name, _)| name.clone())
        .collect();
    super::super::write_change_summary(
        output,
        &path,
        &added,
        &modified,
        &BTreeSet::new(),
        colored,
    )?;
    let proposed_count = additions.len();
    let approved = if auto_approve {
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
    if approved.len() != proposed_count {
        writeln!(output, "Selected server changes:")?;
        let selected_added = approved
            .keys()
            .filter(|name| changes[*name].0.is_none())
            .cloned()
            .collect();
        let selected_modified = approved
            .keys()
            .filter(|name| changes[*name].0.is_some())
            .cloned()
            .collect();
        super::super::write_change_summary(
            output,
            &path,
            &selected_added,
            &selected_modified,
            &BTreeSet::new(),
            colored,
        )?;
    }
    if dry_run {
        writeln!(output, "Would import {} server(s).", approved.len())?;
        return Ok(());
    }
    {
        snapshot.ensure_write_supported()?;
        let backup_path = choose_backup_path(
            &snapshot,
            auto_approve,
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
                let default_text = default.map(|path| path.to_string_lossy().into_owned());
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
        )
        .await?;
        if let Some(backup_path) = backup_path {
            snapshot.create_backup_at(&backup_path).await?;
        }
    }
    // Check that the target still matches the snapshot before
    // atomic replacement.
    let added = snapshot.apply(&approved).await?;
    writeln!(
        output,
        "Imported {added} server(s); existing identical entries were unchanged."
    )?;
    Ok(())
}

/// Switch the entire server set as one approved, atomic configuration change.
#[expect(
    clippy::too_many_arguments,
    reason = "CLI workflow accepts separate command flags"
)]
pub(crate) async fn run_use(
    file: PathBuf,
    config: Option<PathBuf>,
    include_permissions: bool,
    output: &mut impl Write,
    non_interactive: bool,
    colored: bool,
    dry_run: bool,
    auto_approve: bool,
) -> Result<(), AppError> {
    if dry_run {
        tracing::warn!("Dry run; no changes will be committed.");
    }
    let path = config.or_else(default_config).ok_or(Error::ConfigPath)?;
    let snapshot = Snapshot::read(&path).await?;
    let stack: Stack = crate::integrations::selection::Selection::default()
        .read_stack(&file)
        .await?
        .into();
    let (preliminary, _, missing) =
        preliminary_definitions(&snapshot, &stack, include_permissions)?;
    if missing && snapshot.replacement_proposal(&preliminary)?.0 {
        writeln!(
            output,
            "Preliminary changes; missing secrets will be requested for an exact comparison."
        )?;
        let changes = snapshot.preview(&preliminary)?;
        let current = snapshot.servers()?;
        let added = changes
            .iter()
            .filter(|(_, (before, _))| before.is_none())
            .map(|(name, _)| name.clone())
            .collect();
        let modified = changes
            .iter()
            .filter(|(_, (before, _))| before.is_some())
            .map(|(name, _)| name.clone())
            .collect();
        let deleted = current
            .keys()
            .filter(|name| !preliminary.contains_key(*name))
            .cloned()
            .collect();
        super::super::write_change_summary(output, &path, &added, &modified, &deleted, colored)?;
    }
    let interactive = !non_interactive && stdin().is_terminal() && stderr().is_terminal();
    let mut resolver = SecretResolver {
        dry_run,
        interactive: (dry_run || !auto_approve) && interactive,
        preview_missing: false,
        missing_used: false,
        values: BTreeMap::new(),
        cancelled: false,
    };
    let prepared = match &stack {
        Stack::V1(stack) => prepare(stack, |name| resolver.resolve(name)),
    };
    if resolver.cancelled {
        return Err(AppError::ImportApprovalCancelled);
    }
    let mut definitions = prepared?;
    retain_target_policy(&snapshot, &stack, &mut definitions, include_permissions)?;
    if include_permissions {
        apply_permissions(&stack, &mut definitions)?;
    }
    let (changed, _) = snapshot.replacement_proposal(&definitions)?;
    if !changed {
        if !dry_run {
            snapshot.replace_servers(&definitions).await?;
        }
        writeln!(
            output,
            "No changes; the configured MCP servers already match this stack."
        )?;
        return Ok(());
    }
    warn_runtime_bindings(&definitions);
    let changes = snapshot.preview(&definitions)?;
    let current = snapshot.servers()?;
    let added = changes
        .iter()
        .filter(|(_, (before, _))| before.is_none())
        .map(|(name, _)| name.clone())
        .collect();
    let modified = changes
        .iter()
        .filter(|(_, (before, _))| before.is_some())
        .map(|(name, _)| name.clone())
        .collect();
    let deleted = current
        .keys()
        .filter(|name| !definitions.contains_key(*name))
        .cloned()
        .collect();
    writeln!(
        output,
        "Would switch to {} MCP server(s); all servers absent from the stack would be removed.",
        definitions.len()
    )?;
    super::super::write_change_summary(output, &path, &added, &modified, &deleted, colored)?;
    if dry_run {
        return Ok(());
    }
    if !auto_approve {
        if !interactive {
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
    snapshot.ensure_write_supported()?;
    // Switching always backs up the original after validation and whole-set approval.
    let backup = snapshot.default_backup_path().await?;
    snapshot.create_backup_at(&backup).await?;
    snapshot.replace_servers(&definitions).await?;
    writeln!(
        output,
        "Using stack with {} MCP server(s); all other MCP servers were removed.",
        definitions.len()
    )?;
    Ok(())
}

/// Discover numbered backups only after the user requests one. Interactive
/// callers can still choose a custom path when default discovery fails.
async fn choose_backup_path(
    snapshot: &Snapshot,
    auto_approve: bool,
    confirm: impl FnOnce() -> Result<bool, AppError>,
    choose: impl FnOnce(Option<&Path>) -> Result<PathBuf, AppError>,
) -> Result<Option<PathBuf>, AppError> {
    if auto_approve {
        return Ok(Some(snapshot.default_backup_path().await?));
    }
    if !confirm()? {
        return Ok(None);
    }
    let default = snapshot.default_backup_path().await.ok();
    choose(default.as_deref()).map(Some)
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
    use std::fs;

    #[tokio::test]
    async fn unavailable_default_backup_can_be_skipped_or_replaced_with_custom_path() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        // An unrepresentable generation makes numbered discovery fail on every
        // platform, just as a directory that cannot be listed would.
        fs::write(
            directory.path().join("config.toml.~18446744073709551616~"),
            "existing",
        )
        .unwrap();
        let snapshot = Snapshot::read(&path).await.unwrap();
        assert!(snapshot.default_backup_path().await.is_err());
        assert!(
            choose_backup_path(
                &snapshot,
                false,
                || Ok(false),
                |_| { panic!("skipping backup must not ask for a path") }
            )
            .await
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
        .await
        .unwrap();
        assert_eq!(selected, Some(custom));
        assert!(
            choose_backup_path(
                &snapshot,
                true,
                || panic!("automatic backup must not prompt"),
                |_| { panic!("automatic backup must not ask for a path") }
            )
            .await
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
