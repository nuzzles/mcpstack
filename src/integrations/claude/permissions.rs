//! MCP permission rules in Claude Code's separate settings file.
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use super::Error;
use crate::error::AppError;
use crate::integrations::diff_output::write_diff;
use crate::integrations::write_change_rows;
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

    pub fn summarize(
        &self,
        proposed: &Value,
        path: &Path,
        output: &mut impl Write,
        colored: bool,
    ) -> Result<(), AppError> {
        if *proposed == self.config {
            return Ok(());
        }
        let before = rule_modes(&self.config)?;
        let after = rule_modes(proposed)?;
        let mut changes = [Vec::new(), Vec::new(), Vec::new()];
        for (rule, modes) in &after {
            match before.get(rule) {
                None => changes[0].push(format!("{rule:?} ({})", modes.join(", "))),
                Some(old) if old != modes => changes[1].push(format!(
                    "{rule:?} ({} -> {})",
                    old.join(", "),
                    modes.join(", ")
                )),
                Some(_) => {}
            }
        }
        for (rule, modes) in &before {
            if !after.contains_key(rule) {
                changes[2].push(format!("{rule:?} ({})", modes.join(", ")));
            }
        }
        writeln!(
            output,
            "Target settings: {}",
            std::path::absolute(path)?.to_string_lossy().escape_debug()
        )?;
        writeln!(output, "MCP permission changes:")?;
        write_change_rows(output, &changes, colored)?;
        if changes.iter().all(Vec::is_empty) {
            writeln!(
                output,
                "Permission rule order or duplicate entries would be normalized."
            )?;
        }
        Ok(())
    }

    pub fn preview(
        &self,
        proposed: &Value,
        path: &Path,
        output: &mut impl Write,
        colored: bool,
    ) -> Result<(), AppError> {
        if *proposed == self.config {
            return Ok(());
        }
        let original = std::str::from_utf8(self.file.contents()).map_err(|_| Error::Config)?;
        // Use the same serialization as the write path, retaining physical line
        // numbers on both sides even when unrelated settings are omitted.
        let rendered = super::serialized_config(proposed)?;
        let before = if original.is_empty() {
            String::new()
        } else {
            redacted_settings(original, &self.config)?
        };
        let after = redacted_settings(&rendered, proposed)?;
        let label: String = path
            .to_string_lossy()
            .chars()
            .flat_map(|c| {
                if c.is_control() {
                    c.escape_default().collect::<Vec<_>>()
                } else {
                    vec![c]
                }
            })
            .collect();
        let diff = similar::TextDiff::from_lines(&before, &after)
            .unified_diff()
            .context_radius(3)
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
            "Permission settings preview; unrelated settings and non-MCP rules are omitted."
        )?;
        write_diff(output, &diff, colored)?;
        Ok(())
    }
}

fn rule_modes(config: &Value) -> Result<BTreeMap<String, Vec<&'static str>>, Error> {
    let mut modes: BTreeMap<String, Vec<&'static str>> = BTreeMap::new();
    for bucket in ["allow", "ask", "deny"] {
        for rule in rules(config, bucket)?.into_iter().collect::<BTreeSet<_>>() {
            if rule.starts_with("mcp__") {
                modes.entry(rule).or_default().push(bucket);
            }
        }
    }
    Ok(modes)
}

fn redacted_settings(source: &str, config: &Value) -> Result<String, Error> {
    let mut private_rules = BTreeMap::new();
    for bucket in ["allow", "ask", "deny"] {
        for rule in rules(config, bucket)? {
            if !rule.starts_with("mcp__") {
                private_rules.insert(rule, "<unrelated permission rule omitted>".into());
            }
        }
    }
    redacted_fields(source, |name, value| {
        if name != "permissions" {
            return Ok(None);
        }
        redacted_fields(value, |name, value| {
            Ok(matches!(name, "allow" | "ask" | "deny")
                .then(|| super::redact_json_values(value, &private_rules)))
        })
        .map(Some)
    })
}

/// Keep only chosen fields, preserving newlines in hidden fields so diff hunk
/// coordinates still refer to the original and proposed settings files.
fn redacted_fields(
    source: &str,
    render: impl Fn(&str, &str) -> Result<Option<String>, Error>,
) -> Result<String, Error> {
    let mut output = String::new();
    let mut copied = 0;
    for field in super::root_fields(source)? {
        output.push_str(&source[copied..field.start]);
        if let Some(value) = render(&field.name, &source[field.value_start..field.value_end])? {
            output.push_str(&source[field.start..field.value_start]);
            output.push_str(&value);
        } else {
            output.push_str("<unrelated setting omitted>");
            output.extend(
                source[field.start..field.value_end]
                    .chars()
                    .filter(|c| *c == '\n'),
            );
        }
        copied = field.value_end;
    }
    output.push_str(&source[copied..]);
    Ok(output)
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
