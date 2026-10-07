//! Versioned, client-independent operation results with redacted previews.
use std::io::{self, Write};

use serde::Serialize;

use crate::error::{AppError, ErrorCode};

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    Validate,
    Diff,
    Import,
    Export,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Success,
    Failure,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Add,
    Replace,
    Unchanged,
    Validate,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Pending,
    Proposed,
    Approved,
    Skipped,
    Unchanged,
    Applied,
    Validated,
}

#[derive(Serialize)]
pub struct ServerResult {
    pub name: String,
    pub action: Action,
    pub outcome: Outcome,
}

#[derive(Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BackupStatus {
    #[default]
    NotRequested,
    Skipped,
    Created,
    Failed,
}

#[derive(Default, Serialize)]
pub struct Backup {
    pub status: BackupStatus,
    pub path: Option<String>,
}

#[derive(Serialize)]
pub struct Failure {
    code: ErrorCode,
    exit_status: u8,
    message: String,
}

#[derive(Serialize)]
pub struct Report {
    schema_version: u32,
    pub operation: Option<Operation>,
    pub integration: Option<&'static str>,
    status: Status,
    pub dry_run: bool,
    pub stack_schema_version: Option<u64>,
    pub config_path: Option<String>,
    pub servers: Vec<ServerResult>,
    pub backup: Backup,
    /// A redacted, uncolored preview, never raw configuration.
    pub diff: Option<String>,
    error: Option<Failure>,
}

impl Report {
    pub fn new(operation: Option<Operation>) -> Self {
        Self {
            schema_version: 1,
            operation,
            integration: None,
            status: Status::Success,
            dry_run: false,
            stack_schema_version: None,
            config_path: None,
            servers: Vec::new(),
            backup: Backup::default(),
            diff: None,
            error: None,
        }
    }

    pub fn fail(&mut self, error: &AppError) {
        self.status = Status::Failure;
        let code = error.code();
        self.error = Some(Failure {
            code,
            exit_status: code.as_u8(),
            message: error.to_string(),
        });
    }

    pub fn write_json(&self, output: &mut impl Write) -> io::Result<()> {
        // Serialize first: never intentionally emit a partially built document.
        let mut bytes = serde_json::to_vec(self).map_err(io::Error::other)?;
        bytes.push(b'\n');
        output.write_all(&bytes)
    }
}

/// Human output streams as before. JSON operations collect typed metadata and
/// suppress human summaries; the caller writes exactly one report at the end.
pub struct OperationOutput<'a, W: Write> {
    writer: &'a mut W,
    pub json: bool,
    pub report: Report,
}

impl<'a, W: Write> OperationOutput<'a, W> {
    pub fn new(writer: &'a mut W, json: bool, operation: Option<Operation>) -> Self {
        Self {
            writer,
            json,
            report: Report::new(operation),
        }
    }

    pub fn finish(&mut self, result: &Result<(), AppError>) -> Result<(), AppError> {
        if let Err(error) = result {
            self.report.fail(error);
        }
        if self.json {
            self.report.write_json(self.writer)?;
        }
        Ok(())
    }
}

impl<W: Write> Write for OperationOutput<'_, W> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.json {
            Ok(bytes.len())
        } else {
            self.writer.write(bytes)
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        if self.json {
            Ok(())
        } else {
            self.writer.flush()
        }
    }
}
