//! Exact server-name selection shared by config and stack operations.
use std::collections::BTreeSet;

use clap::Args;
use thiserror::Error;

use crate::schema::StackV1;

#[derive(Args, Clone, Default)]
pub struct Selection {
    /// Select a server by exact name; repeat to select multiple servers.
    #[arg(long = "server", value_name = "NAME")]
    names: Vec<String>,
}

impl Selection {
    pub async fn read_stack(
        &self,
        file: &std::path::Path,
    ) -> Result<StackV1, crate::error::AppError> {
        let document = tokio::fs::read_to_string(file)
            .await
            .map_err(crate::error::AppError::StackRead)?;
        let crate::schema::Stack::V1(mut stack) = crate::schema::Stack::from_yaml(&document)?;
        self.apply(&mut stack)?;
        Ok(stack)
    }

    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }

    pub fn includes(&self, name: &str) -> bool {
        self.is_empty() || self.names.iter().any(|selected| selected == name)
    }

    pub fn validate<'a>(&self, available: impl Iterator<Item = &'a str>) -> Result<(), Error> {
        let available: BTreeSet<_> = available.collect();
        let missing: BTreeSet<_> = self
            .names
            .iter()
            .filter(|name| !available.contains(name.as_str()))
            .map(|name| format!("{name:?}"))
            .collect();
        if missing.is_empty() {
            Ok(())
        } else {
            Err(Error::Unknown(
                missing.into_iter().collect::<Vec<_>>().join(", "),
            ))
        }
    }

    pub fn apply(&self, stack: &mut StackV1) -> Result<(), Error> {
        self.validate(stack.servers.keys().map(String::as_str))?;
        stack.servers.retain(|name, _| self.includes(name));
        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("Unknown MCP server selection(s): {0}. Names must match exactly.")]
    Unknown(String),
    #[error("Selected MCP server {0:?} is excluded by default. Add --all or its --allow-* flag.")]
    Excluded(String),
}
