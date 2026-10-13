//! Shared ANSI formatting for client configuration diffs.
use std::io::{self, Write};
use std::path::Path;

/// Keep ordinary path spelling intact, including Windows separators, while
/// preventing filenames from injecting terminal controls or diff header lines.
pub(super) fn path_label(path: &Path) -> String {
    path.to_string_lossy()
        .chars()
        .flat_map(|c| {
            if c.is_control() {
                c.escape_default().collect::<Vec<_>>()
            } else {
                vec![c]
            }
        })
        .collect()
}

pub(super) fn write_diff(output: &mut impl Write, diff: &str, colored: bool) -> io::Result<()> {
    if !colored {
        return write!(output, "{diff}");
    }
    for line in diff.lines() {
        let color =
            if line.starts_with("--- ") || line.starts_with("+++ ") || line.starts_with("@@ ") {
                Some(36) // cyan headers
            } else if line.starts_with('+') {
                Some(32) // green additions
            } else if line.starts_with('-') {
                Some(31) // red removals
            } else {
                None
            };
        if let Some(color) = color {
            writeln!(output, "\x1b[{color}m{line}\x1b[0m")?;
        } else {
            writeln!(output, "{line}")?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_escape_controls_without_rewriting_ordinary_paths() {
        let windows = "C:\\tools\\café \\\"quoted\\\"\\config.toml";
        assert_eq!(path_label(Path::new(windows)), windows);
        assert_eq!(
            path_label(Path::new("config\x1b[2J\nforged\r\t\u{85}.json")),
            "config\\u{1b}[2J\\nforged\\r\\t\\u{85}.json"
        );
    }
}
