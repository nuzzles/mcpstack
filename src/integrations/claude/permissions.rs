//! MCP permission rules in Claude Code's separate settings file.
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use super::Error;
use crate::io;
use crate::schema::v1::{ApprovalMode, ConfigValue, Permissions};
use crate::schema::{Server, StackV1};

pub(super) fn path(explicit: Option<PathBuf>, config: Option<&PathBuf>) -> Result<PathBuf, Error> {
    if let Some(path) = explicit {
        return Ok(path);
    }
    if let Some(path) = config {
        return Ok(path
            .parent()
            .unwrap_or(std::path::Path::new("."))
            .join("settings.json"));
    }
    directories::BaseDirs::new()
        .map(|dirs| dirs.home_dir().join(".claude/settings.json"))
        .ok_or(Error::Path)
}

pub(super) struct Snapshot {
    pub file: io::Snapshot,
    pub config: Value,
}

impl Snapshot {
    pub async fn read(path: &Path) -> Result<Self, Error> {
        let file = io::Snapshot::read(path).await.map_err(|_| Error::Read)?;
        let config = if file.contents().is_empty() {
            Value::Object(Map::new())
        } else {
            serde_json::to_value(
                serde_json::from_slice::<ConfigValue>(file.contents())
                    .map_err(|_| Error::Config)?,
            )
            .map_err(|_| Error::Config)?
        };
        if !config.is_object() {
            return Err(Error::Config);
        }
        for bucket in ["allow", "ask", "deny"] {
            let _ = rules(&config, bucket)?;
        }
        Ok(Self { file, config })
    }

    pub fn for_server(&self, name: &str) -> Result<Option<Permissions>, Error> {
        let prefix = format!("mcp__{name}");
        let select = |bucket| -> Result<Vec<String>, Error> {
            Ok(rules(&self.config, bucket)?
                .into_iter()
                .filter(|rule| rule == &prefix || rule.starts_with(&format!("{prefix}__")))
                .collect())
        };
        let allow = select("allow")?;
        let ask = select("ask")?;
        let deny = select("deny")?;
        Ok(Some(Permissions::Claude { allow, ask, deny }))
    }

    pub fn proposed(&self, stack: &StackV1, names: &BTreeSet<String>) -> Result<Value, Error> {
        let mut proposed = self.config.clone();
        for bucket in ["allow", "ask", "deny"] {
            let mut entries = rules(&proposed, bucket)?;
            for (name, server) in &stack.servers {
                if !names.contains(name) {
                    continue;
                }
                let Server::Portable {
                    permissions: Some(policy),
                    ..
                } = server
                else {
                    continue;
                };
                let prefix = format!("mcp__{name}");
                match &**policy {
                    Permissions::Claude { .. } => entries.retain(|rule| {
                        rule != &prefix && !rule.starts_with(&format!("{prefix}__"))
                    }),
                    Permissions::Codex {
                        default_mode,
                        tools,
                    } => {
                        let map_default = default_mode.is_some_and(representable)
                            && tools.values().all(|mode| representable(*mode))
                            && !(matches!(default_mode, Some(ApprovalMode::Prompt))
                                && tools
                                    .values()
                                    .any(|mode| matches!(mode, ApprovalMode::Approve)));
                        entries.retain(|rule| {
                            if (rule == &prefix || rule == &format!("{prefix}__*")) && map_default {
                                return false;
                            }
                            !tools.iter().any(|(tool, mode)| {
                                representable(*mode)
                                    && !(matches!(default_mode, Some(ApprovalMode::Prompt))
                                        && matches!(mode, ApprovalMode::Approve))
                                    && rule == &format!("{prefix}__{tool}")
                            })
                        });
                    }
                }
            }
            for (name, server) in &stack.servers {
                if !names.contains(name) {
                    continue;
                }
                let Server::Portable {
                    permissions: Some(policy),
                    ..
                } = server
                else {
                    continue;
                };
                match &**policy {
                    Permissions::Claude { allow, ask, deny } => {
                        entries.extend(
                            match bucket {
                                "allow" => allow,
                                "ask" => ask,
                                _ => deny,
                            }
                            .iter()
                            .cloned(),
                        );
                    }
                    Permissions::Codex {
                        default_mode,
                        tools,
                    } => {
                        let unrepresentable_tools =
                            tools.values().any(|mode| !representable(*mode));
                        let conflicting_override =
                            matches!(default_mode, Some(ApprovalMode::Prompt))
                                && tools
                                    .values()
                                    .any(|mode| matches!(mode, ApprovalMode::Approve));
                        if let Some(mode) = default_mode {
                            if representable(*mode)
                                && !unrepresentable_tools
                                && !conflicting_override
                            {
                                add_codex_rule(
                                    &mut entries,
                                    bucket,
                                    &format!("mcp__{name}__*"),
                                    *mode,
                                );
                            } else if bucket == "allow" {
                                tracing::warn!(
                                    "Skipping Codex default approval mode for {name:?}: Claude Code cannot reproduce auto or writes mode or this combination of tool overrides. MCP servers will still be applied."
                                );
                            }
                        }
                        for (tool, mode) in tools {
                            if !representable(*mode) {
                                if bucket == "allow" {
                                    tracing::warn!(
                                        "Skipping Codex approval mode for {name:?}.{tool:?}: Claude Code cannot reproduce auto or writes mode. MCP servers will still be applied."
                                    );
                                }
                                continue;
                            }
                            if matches!(
                                (default_mode, mode),
                                (Some(ApprovalMode::Prompt), ApprovalMode::Approve)
                            ) {
                                if bucket == "allow" {
                                    tracing::warn!(
                                        "Skipping Codex approval override for {name:?}.{tool:?}: Claude's server-wide ask rule takes precedence."
                                    );
                                }
                                continue;
                            }
                            add_codex_rule(
                                &mut entries,
                                bucket,
                                &format!("mcp__{name}__{tool}"),
                                *mode,
                            );
                        }
                    }
                }
            }
            let mut unique = BTreeSet::new();
            entries.retain(|rule| unique.insert(rule.clone()));
            set_rules(&mut proposed, bucket, entries)?;
        }
        Ok(proposed)
    }
}

fn representable(mode: ApprovalMode) -> bool {
    matches!(mode, ApprovalMode::Approve | ApprovalMode::Prompt)
}

fn add_codex_rule(entries: &mut Vec<String>, bucket: &str, rule: &str, mode: ApprovalMode) {
    match mode {
        ApprovalMode::Approve if bucket == "allow" => entries.push(rule.into()),
        ApprovalMode::Prompt if bucket == "ask" => entries.push(rule.into()),
        ApprovalMode::Auto | ApprovalMode::Writes if bucket == "allow" => tracing::warn!(
            "Skipping Codex approval mode for {rule}: Claude Code has no exact equivalent."
        ),
        _ => {}
    }
}

fn rules(config: &Value, bucket: &str) -> Result<Vec<String>, Error> {
    let value = config.get("permissions").and_then(|v| v.get(bucket));
    match value {
        None => Ok(Vec::new()),
        Some(Value::Array(values)) => values
            .iter()
            .map(|v| v.as_str().map(str::to_owned).ok_or(Error::Config))
            .collect(),
        _ => Err(Error::Config),
    }
}

fn set_rules(config: &mut Value, bucket: &str, entries: Vec<String>) -> Result<(), Error> {
    let root = config.as_object_mut().ok_or(Error::Config)?;
    if entries.is_empty() && !root.contains_key("permissions") {
        return Ok(());
    }
    let permissions = root
        .entry("permissions")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or(Error::Config)?;
    if !entries.is_empty() || permissions.contains_key(bucket) {
        permissions.insert(
            bucket.into(),
            Value::Array(entries.into_iter().map(Value::String).collect()),
        );
    }
    Ok(())
}
