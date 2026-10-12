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
    allow_computer_use: bool,
}

#[derive(Args)]
pub struct CodexFilterArgs {
    #[command(flatten)]
    common: CommonFilterArgs,
    /// Include the otherwise skipped node_repl server.
    #[arg(long)]
    allow_node_repl: bool,
}

impl FilterArgs for CodexFilterArgs {
    fn filter(&self) -> ServerFilter {
        ServerFilter {
            selection: self.common.selection.clone(),
            all: self.common.all,
            computer_use: self.common.allow_computer_use,
            node_repl: self.allow_node_repl,
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
    allow_workspace: bool,
    /// Include the otherwise skipped claude-in-chrome server.
    #[arg(long)]
    allow_claude_in_chrome: bool,
    /// Include the otherwise skipped Claude Preview server.
    #[arg(long)]
    allow_claude_preview: bool,
    /// Include the otherwise skipped Claude Browser server.
    #[arg(long)]
    allow_claude_browser: bool,
}

impl FilterArgs for ClaudeFilterArgs {
    fn filter(&self) -> ServerFilter {
        ServerFilter {
            selection: self.common.selection.clone(),
            all: self.common.all,
            computer_use: self.common.allow_computer_use,
            workspace: self.allow_workspace,
            claude_in_chrome: self.allow_claude_in_chrome,
            claude_preview: self.allow_claude_preview,
            claude_browser: self.allow_claude_browser,
            node_repl: true,
        }
    }
}
