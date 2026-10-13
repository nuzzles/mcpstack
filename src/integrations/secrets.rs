use dialoguer::Password;
use std::collections::BTreeMap;
use std::env;

/// Cache each reference so repeated uses ask only once. All comparisons use
/// real values; dry-run never proceeds to backup or file writes.
pub(crate) struct SecretResolver {
    pub(crate) dry_run: bool,
    pub(crate) interactive: bool,
    pub(crate) preview_missing: bool,
    pub(crate) missing_used: bool,
    pub(crate) values: BTreeMap<String, Option<String>>,
    pub(crate) cancelled: bool,
}

impl SecretResolver {
    pub(crate) fn resolve(&mut self, name: &str) -> Option<String> {
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
            if self.preview_missing {
                self.missing_used = true;
                value = Some("https://example.invalid/mcpstack-secret-preview".into());
            } else if self.interactive {
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

/// Redact explicitly resolved secrets even when their field names are not sensitive.
pub(super) fn redact_resolved(value: &mut serde_json::Value, secrets: &[String]) {
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
