//! Shared ANSI formatting for client configuration diffs.
use std::io::{self, Write};

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
