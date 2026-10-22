use clap::Args;

/// Claude Code skips user-configured MCP servers with these built-in names.
pub(crate) fn is_reserved(name: &str) -> bool {
    ServerFilter {
        node_repl: true,
        ..Default::default()
    }
    .skip(name)
}

#[derive(Clone, Default)]
pub struct ServerFilter {
    pub selection: super::selection::Selection,
    pub all: bool,
    pub workspace: bool,
    pub claude_in_chrome: bool,
    pub computer_use: bool,
    pub claude_preview: bool,
    pub claude_browser: bool,
    pub node_repl: bool,
}

impl ServerFilter {
    pub(crate) fn for_harness(harness: super::harness::Harness) -> Self {
        match harness {
            super::harness::Harness::Codex => Self {
                workspace: true,
                claude_in_chrome: true,
                claude_preview: true,
                claude_browser: true,
                ..Default::default()
            },
            super::harness::Harness::Claude => Self {
                node_repl: true,
                ..Default::default()
            },
        }
    }

    pub fn validate<'a>(
        &self,
        names: impl Iterator<Item = &'a str>,
    ) -> Result<(), super::selection::Error> {
        let names: Vec<_> = names.collect();
        self.selection.validate(names.iter().copied())?;
        for name in names {
            if !self.selection.is_empty() && self.selection.includes(name) && self.excluded(name) {
                return Err(super::selection::Error::Excluded(name.to_owned()));
            }
        }
        Ok(())
    }

    pub fn skip(&self, name: &str) -> bool {
        !self.selection.includes(name) || self.excluded(name)
    }

    fn excluded(&self, name: &str) -> bool {
        if self.all {
            return false;
        }
        match name {
            "workspace" => !self.workspace,
            "claude-in-chrome" => !self.claude_in_chrome,
            "computer-use" => !self.computer_use,
            "Claude Preview" => !self.claude_preview,
            "Claude Browser" => !self.claude_browser,
            "node_repl" => !self.node_repl,
            _ => false,
        }
    }
}

/// The same client-specific flags and defaults drive list, show, and export.
pub trait FilterArgs {
    fn filter(&self) -> ServerFilter;
}

#[derive(Args)]
struct CommonFilterArgs {
    #[command(flatten)]
    selection: super::selection::Selection,
    /// Include every normally skipped server.
    #[arg(long)]
    all: bool,
    /// Include the otherwise skipped computer-use server.
    #[arg(long)]
    include_computer_use: bool,
}

#[derive(Args)]
pub struct CodexFilterArgs {
    #[command(flatten)]
    common: CommonFilterArgs,
    /// Include the otherwise skipped node_repl server.
    #[arg(long)]
    include_node_repl: bool,
}

impl FilterArgs for CodexFilterArgs {
    fn filter(&self) -> ServerFilter {
        ServerFilter {
            selection: self.common.selection.clone(),
            all: self.common.all,
            computer_use: self.common.include_computer_use,
            node_repl: self.include_node_repl,
            workspace: true,
            claude_in_chrome: true,
            claude_preview: true,
            claude_browser: true,
        }
    }
}

#[derive(Args)]
pub struct ClaudeFilterArgs {
    #[command(flatten)]
    common: CommonFilterArgs,
    /// Include the otherwise skipped workspace server.
    #[arg(long)]
    include_workspace: bool,
    /// Include the otherwise skipped claude-in-chrome server.
    #[arg(long)]
    include_claude_in_chrome: bool,
    /// Include the otherwise skipped Claude Preview server.
    #[arg(long)]
    include_claude_preview: bool,
    /// Include the otherwise skipped Claude Browser server.
    #[arg(long)]
    include_claude_browser: bool,
}

impl FilterArgs for ClaudeFilterArgs {
    fn filter(&self) -> ServerFilter {
        ServerFilter {
            selection: self.common.selection.clone(),
            all: self.common.all,
            computer_use: self.common.include_computer_use,
            workspace: self.include_workspace,
            claude_in_chrome: self.include_claude_in_chrome,
            claude_preview: self.include_claude_preview,
            claude_browser: self.include_claude_browser,
            node_repl: true,
        }
    }
}
