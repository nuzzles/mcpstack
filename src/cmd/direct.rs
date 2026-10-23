//! Interactive client commands without a harness namespace.
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use clap::Args;
use dialoguer::Input;

use crate::cmd::export;
use crate::cmd::show::Show as ShowCommand;
use crate::error::AppError;
use crate::integrations::harness::{self, Harness};
use crate::integrations::reserved::ServerFilter;
use crate::integrations::selection::Selection;
use crate::integrations::{Claude, ClientAdapter, Codex, ExportArgs, StackArgs};
use crate::stack_source::StackSource;

#[derive(Args)]
struct OneHarness {
    /// Choose a harness without prompting.
    #[arg(long, value_enum, value_name = "HARNESS")]
    harness: Option<Harness>,
}

impl OneHarness {
    fn select(self, non_interactive: bool) -> Result<Option<Harness>, AppError> {
        harness::select_one(self.harness, non_interactive)
    }
}

#[derive(Args)]
struct ManyHarnesses {
    /// Choose a harness without prompting; repeat for multiple harnesses.
    #[arg(long = "harness", value_enum, value_name = "HARNESS")]
    harnesses: Vec<Harness>,
}

impl ManyHarnesses {
    fn select(self, non_interactive: bool) -> Result<Vec<Harness>, AppError> {
        harness::select_many(self.harnesses, non_interactive)
    }
}

#[derive(Args)]
struct Source {
    /// YAML stack file path or HTTP(S) URL; prompted when omitted.
    #[arg(value_name = "SOURCE")]
    source: Option<StackSource>,
}

impl Source {
    fn resolve(self, non_interactive: bool) -> Result<StackSource, AppError> {
        if let Some(source) = self.source {
            return Ok(source);
        }
        if !harness::can_prompt(non_interactive) {
            return Err(AppError::StackSourceRequired);
        }
        let text: String = Input::new()
            .with_prompt("Stack file path or HTTP(S) URL")
            .report(false)
            .interact_text()
            .map_err(|_| AppError::StackSourcePrompt)?;
        StackSource::parse(Path::new(&text))
    }
}

#[derive(Args)]
struct Config {
    /// Read or write this client config instead of its default; requires one harness.
    #[arg(long, value_name = "PATH")]
    config: Option<PathBuf>,
}

impl Config {
    fn for_many(&self, harnesses: &[Harness]) -> Result<Option<PathBuf>, AppError> {
        if self.config.is_some() && harnesses.len() != 1 {
            return Err(AppError::ConfigHarnessAmbiguous);
        }
        Ok(self.config.clone())
    }
}

fn destination(file: Option<PathBuf>, non_interactive: bool) -> Result<PathBuf, AppError> {
    if let Some(file) = file {
        return Ok(file);
    }
    if !harness::can_prompt(non_interactive) {
        return Err(AppError::ExportDestinationRequired);
    }
    let text: String = Input::new()
        .with_prompt("Export file (.yaml or .yml)")
        .report(false)
        .interact_text()
        .map_err(|_| AppError::ExportDestinationPrompt)?;
    Ok(PathBuf::from(text))
}

fn filter(harness: Harness, selection: Selection, all: bool) -> ServerFilter {
    let mut filter = ServerFilter::for_harness(harness);
    filter.selection = selection;
    filter.all = all;
    filter
}

struct HarnessOutput<'a, W> {
    output: &'a mut W,
    harness: Harness,
    labeled: bool,
}

impl<'a, W: Write> HarnessOutput<'a, W> {
    fn new(output: &'a mut W, harness: Harness) -> Self {
        Self {
            output,
            harness,
            labeled: false,
        }
    }

    fn finish(&mut self) -> io::Result<()> {
        if !self.labeled {
            writeln!(self.output, "{}:", self.harness.name())?;
            self.labeled = true;
        }
        Ok(())
    }
}

impl<W: Write> Write for HarnessOutput<'_, W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if !buf.is_empty() {
            self.finish()?;
        }
        self.output.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.output.flush()
    }
}

#[derive(Args)]
/// List configured MCP servers in one harness.
pub struct List {
    #[command(flatten)]
    harness: OneHarness,
    #[command(flatten)]
    config: Config,
    #[command(flatten)]
    selection: Selection,
    /// Include every normally skipped server.
    #[arg(long)]
    all: bool,
}

impl List {
    pub async fn run(
        self,
        output: &mut impl Write,
        non_interactive: bool,
        colored: bool,
    ) -> Result<(), AppError> {
        let Some(harness) = self.harness.select(non_interactive)? else {
            return Ok(());
        };
        let mut labeled = HarnessOutput::new(output, harness);
        let filter = filter(harness, self.selection, self.all);
        match harness {
            Harness::Claude => {
                Claude
                    .list(self.config.config, filter, &mut labeled, colored)
                    .await
            }
            Harness::Codex => {
                Codex
                    .list(self.config.config, filter, &mut labeled, colored)
                    .await
            }
        }?;
        labeled.finish()?;
        Ok(())
    }
}

#[derive(Args)]
/// Print a portable YAML stack from one harness.
pub struct Show {
    #[command(flatten)]
    harness: OneHarness,
    #[command(flatten)]
    config: Config,
    #[command(flatten)]
    selection: Selection,
    /// Include every normally skipped server.
    #[arg(long)]
    all: bool,
    /// Include per-server MCP permissions and tool filters.
    #[arg(long)]
    include_permissions: bool,
    /// Preserve literal credentials in the stack.
    #[arg(long)]
    expose_secrets: bool,
}

impl Show {
    pub async fn run(self, output: &mut impl Write, non_interactive: bool) -> Result<(), AppError> {
        let Some(harness) = self.harness.select(non_interactive)? else {
            return Ok(());
        };
        let args = ExportArgs::for_harness(
            harness,
            self.config.config,
            self.selection,
            self.all,
            self.include_permissions,
        );
        match harness {
            Harness::Claude => {
                Claude
                    .show(args, output, non_interactive, self.expose_secrets)
                    .await
            }
            Harness::Codex => {
                Codex
                    .show(args, output, non_interactive, self.expose_secrets)
                    .await
            }
        }
    }
}

#[derive(Args)]
/// Export a portable YAML stack from one harness.
pub struct Export {
    #[command(flatten)]
    harness: OneHarness,
    #[command(flatten)]
    config: Config,
    /// Destination YAML stack file; prompted when omitted.
    #[arg(value_name = "FILE")]
    file: Option<PathBuf>,
    /// Replace an existing stack file without prompting.
    #[arg(long)]
    overwrite: bool,
    #[command(flatten)]
    selection: Selection,
    /// Include every normally skipped server.
    #[arg(long)]
    all: bool,
    /// Include per-server MCP permissions and tool filters.
    #[arg(long)]
    include_permissions: bool,
    /// Preserve literal credentials in the stack.
    #[arg(long)]
    expose_secrets: bool,
}

impl Export {
    pub async fn run(self, output: &mut impl Write, non_interactive: bool) -> Result<(), AppError> {
        let Some(harness) = self.harness.select(non_interactive)? else {
            return Ok(());
        };
        let file = destination(self.file, non_interactive)?;
        let args = ExportArgs::for_harness(
            harness,
            self.config.config,
            self.selection,
            self.all,
            self.include_permissions,
        );
        let show = ShowCommand::from_args(args, self.expose_secrets);
        match harness {
            Harness::Claude => {
                export::run_with(file, self.overwrite, show, &Claude, output, non_interactive).await
            }
            Harness::Codex => {
                export::run_with(file, self.overwrite, show, &Codex, output, non_interactive).await
            }
        }
    }
}

#[derive(Args)]
/// Preview a stack against one harness.
pub struct Diff {
    #[command(flatten)]
    harness: OneHarness,
    #[command(flatten)]
    config: Config,
    #[command(flatten)]
    source: Source,
    #[command(flatten)]
    selection: Selection,
    /// Compare permissions and tool filters.
    #[arg(long)]
    include_permissions: bool,
}

impl Diff {
    pub async fn run(
        self,
        output: &mut impl Write,
        non_interactive: bool,
        colored: bool,
    ) -> Result<(), AppError> {
        let source = self.source.resolve(non_interactive)?;
        let Some(harness) = self.harness.select(non_interactive)? else {
            return Ok(());
        };
        let mut labeled = HarnessOutput::new(output, harness);
        let args = StackArgs::for_harnessless(
            source,
            self.config.config,
            self.selection,
            self.include_permissions,
        );
        match harness {
            Harness::Claude => {
                Claude
                    .diff(args, &mut labeled, non_interactive, colored)
                    .await
            }
            Harness::Codex => {
                Codex
                    .diff(args, &mut labeled, non_interactive, colored)
                    .await
            }
        }?;
        labeled.finish()?;
        Ok(())
    }
}

#[derive(Args)]
/// Merge a stack into selected harnesses.
pub struct Import {
    #[command(flatten)]
    harnesses: ManyHarnesses,
    #[command(flatten)]
    config: Config,
    #[command(flatten)]
    source: Source,
    #[command(flatten)]
    selection: Selection,
    /// Summarize changes without writing files.
    #[arg(long)]
    dry_run: bool,
    /// Approve all selected server and permission changes without prompting.
    #[arg(short = 'y', long)]
    auto_approve: bool,
    /// Include permissions and tool filters.
    #[arg(long)]
    include_permissions: bool,
}

pub(crate) enum ImportOutcome {
    NoHarnesses,
    Ran,
}

impl Import {
    pub fn is_dry_run(&self) -> bool {
        self.dry_run
    }

    pub async fn run(
        self,
        output: &mut impl Write,
        non_interactive: bool,
        colored: bool,
    ) -> Result<ImportOutcome, AppError> {
        let harnesses = self.harnesses.select(non_interactive)?;
        if harnesses.is_empty() {
            writeln!(output, "No harnesses selected; nothing was imported.")?;
            return Ok(ImportOutcome::NoHarnesses);
        }
        let config = self.config.for_many(&harnesses)?;
        let source = self.source.resolve(non_interactive)?;
        for harness in harnesses {
            let mut labeled = HarnessOutput::new(output, harness);
            let args = StackArgs::for_harnessless(
                source.clone(),
                config.clone(),
                self.selection.clone(),
                self.include_permissions,
            );
            match harness {
                Harness::Claude => {
                    Claude
                        .import(
                            args,
                            &mut labeled,
                            non_interactive,
                            colored,
                            self.dry_run,
                            self.auto_approve,
                        )
                        .await?
                }
                Harness::Codex => {
                    Codex
                        .import(
                            args,
                            &mut labeled,
                            non_interactive,
                            colored,
                            self.dry_run,
                            self.auto_approve,
                        )
                        .await?
                }
            }
            labeled.finish()?;
        }
        Ok(ImportOutcome::Ran)
    }
}

#[derive(Args)]
/// Replace MCP servers in one harness with a stack.
pub struct Use {
    #[command(flatten)]
    harness: OneHarness,
    #[command(flatten)]
    config: Config,
    #[command(flatten)]
    source: Source,
    /// Summarize additions, replacements, and removals without writing files.
    #[arg(long)]
    dry_run: bool,
    /// Approve replacing the entire server set and create a numbered backup.
    #[arg(short = 'y', long)]
    auto_approve: bool,
    /// Include permissions and tool filters.
    #[arg(long)]
    include_permissions: bool,
}

impl Use {
    pub fn is_dry_run(&self) -> bool {
        self.dry_run
    }

    pub async fn run(
        self,
        output: &mut impl Write,
        non_interactive: bool,
        colored: bool,
    ) -> Result<(), AppError> {
        let source = self.source.resolve(non_interactive)?;
        let Some(harness) = self.harness.select(non_interactive)? else {
            return Ok(());
        };
        let mut labeled = HarnessOutput::new(output, harness);
        let args = StackArgs::for_harnessless(
            source,
            self.config.config,
            Selection::default(),
            self.include_permissions,
        );
        match harness {
            Harness::Claude => {
                Claude
                    .use_stack(
                        args,
                        &mut labeled,
                        non_interactive,
                        colored,
                        self.dry_run,
                        self.auto_approve,
                    )
                    .await
            }
            Harness::Codex => {
                Codex
                    .use_stack(
                        args,
                        &mut labeled,
                        non_interactive,
                        colored,
                        self.dry_run,
                        self.auto_approve,
                    )
                    .await
            }
        }?;
        labeled.finish()?;
        Ok(())
    }
}
