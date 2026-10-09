/// Claude Code skips user-configured MCP servers with these built-in names.
/// Keep the export filter shared so stacks exported from Codex can be used by
/// Claude Code without carrying names it cannot load.
pub(crate) fn is_reserved(name: &str) -> bool {
    matches!(
        name,
        "workspace" | "claude-in-chrome" | "computer-use" | "Claude Preview" | "Claude Browser"
    )
}
