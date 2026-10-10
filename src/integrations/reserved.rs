/// Claude Code skips user-configured MCP servers with these built-in names.
/// Keep the export filter shared so stacks exported from Codex can be used by
/// Claude Code without carrying names it cannot load.
pub(crate) fn is_reserved(name: &str) -> bool {
    matches!(
        name,
        "workspace" | "claude-in-chrome" | "computer-use" | "Claude Preview" | "Claude Browser"
    )
}

#[derive(Clone, Copy, Default)]
pub(crate) struct ExportFilter {
    pub workspace: bool,
    pub claude_in_chrome: bool,
    pub computer_use: bool,
    pub claude_preview: bool,
    pub claude_browser: bool,
    pub node_repl: bool,
}

impl ExportFilter {
    pub fn skip(self, name: &str) -> bool {
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
