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

/// Claude's native rules cannot disambiguate a server prefix from a tool
/// name containing the same separator. Reject overlapping namespaces before
/// exporting or applying any policy, including against unselected servers.
pub(super) fn validate_names<'a>(names: impl Iterator<Item = &'a str>) -> Result<(), Error> {
    let names: BTreeSet<_> = names.collect();
    for name in &names {
        if names.iter().any(|other| {
            other
                .strip_prefix(name)
                .is_some_and(|suffix| suffix.starts_with("__"))
        }) {
            return Err(Error::Permissions(
                (*name).into(),
                "another server name starts with this name followed by '__'; rename one server to avoid ambiguous permission rules",
            ));
        }
    }
    Ok(())
}

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
            rules(&self.config, bucket)?
                .into_iter()
                .filter_map(|rule| {
                    if rule == prefix {
                        Some(Ok("*".into()))
                    } else {
                        rule.strip_prefix(&format!("{prefix}__")).map(|tool| {
                            if tool.is_empty() {
                                Err(Error::Config)
                            } else {
                                Ok(tool.to_owned())
                            }
                        })
                    }
                })
                .collect()
        };
        Ok(Some(Permissions {
            allow: select("allow")?,
            ask: select("ask")?,
            deny: select("deny")?,
            ..Permissions::default()
        }))
    }

    pub fn proposed(&self, stack: &StackV1, names: &BTreeSet<String>) -> Result<Value, Error> {
        let mut policies = Vec::new();
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
            // Convert every selected policy before preparing any file writes.
            policies.push((format!("mcp__{name}"), native_rules(name, policy)?));
        }
        let mut proposed = self.config.clone();
        for (index, bucket) in ["allow", "ask", "deny"].into_iter().enumerate() {
            let mut entries = rules(&proposed, bucket)?;
            for (prefix, policy) in &policies {
                let bare_server_rule = entries.contains(prefix);
                entries.retain(|rule| rule != prefix && !rule.starts_with(&format!("{prefix}__")));
                entries.extend(policy[index].iter().map(|tool| {
                    if tool == "*" && bare_server_rule {
                        prefix.clone()
                    } else {
                        format!("{prefix}__{tool}")
                    }
                }));
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

fn native_rules(name: &str, policy: &Permissions) -> Result<[Vec<String>; 3], Error> {
    let fail = |reason| Error::Permissions(name.into(), reason);
    if !policy.auto.is_empty()
        || !policy.writes.is_empty()
        || matches!(
            policy.default_mode,
            Some(ApprovalMode::Auto | ApprovalMode::Writes)
        )
    {
        return Err(fail(
            "auto and writes approval decisions have no equivalent",
        ));
    }
    let mut buckets = [
        policy.allow.clone(),
        policy.ask.clone(),
        policy.deny.clone(),
    ];
    if let Some(mode) = policy.default_mode {
        // A shared default is a fallback; Claude's server-wide ask rule also
        // overrides allow rules for individual tools, so that case is lossy.
        if mode == ApprovalMode::Ask && !policy.allow.is_empty() {
            return Err(fail(
                "a default ask decision cannot preserve tool allow overrides",
            ));
        }
        if (mode == ApprovalMode::Allow && policy.ask.iter().any(|tool| tool == "*"))
            || policy.deny.iter().any(|tool| tool == "*")
        {
            return Err(fail(
                "a server-wide rule would override the fallback decision",
            ));
        }
        let index = if mode == ApprovalMode::Allow { 0 } else { 1 };
        if !buckets[index].iter().any(|tool| tool == "*") {
            buckets[index].push("*".into());
        }
    }
    Ok(buckets)
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
