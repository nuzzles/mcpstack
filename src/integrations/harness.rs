//! Harness choices for commands without a client namespace.
use std::env;
use std::fs;
use std::io::{IsTerminal, stderr, stdin};
use std::path::Path;

use clap::ValueEnum;
use dialoguer::{MultiSelect, Select};

use super::{claude, codex};
use crate::error::AppError;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, ValueEnum)]
pub(crate) enum Harness {
    Claude,
    Codex,
}

impl Harness {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Claude => "Claude Code",
            Self::Codex => "Codex",
        }
    }

    fn executable(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
        }
    }

    fn installed(self) -> bool {
        let config_exists = match self {
            Self::Claude => claude::path(None).is_ok_and(|path| path.is_file()),
            Self::Codex => codex::default_config().is_some_and(|path| path.is_file()),
        };
        config_exists || executable_on_path(self.executable())
    }
}

#[derive(Clone, Copy)]
struct OptionItem {
    harness: Harness,
    installed: bool,
}

impl OptionItem {
    fn label(self) -> String {
        if self.installed {
            format!("{} (installed)", self.harness.name())
        } else {
            self.harness.name().to_owned()
        }
    }
}

fn ordered_options(installed: impl Fn(Harness) -> bool) -> Vec<OptionItem> {
    let mut options: Vec<_> = [Harness::Claude, Harness::Codex]
        .into_iter()
        .map(|harness| OptionItem {
            harness,
            installed: installed(harness),
        })
        .collect();
    options.sort_by_key(|option| (!option.installed, option.harness.name()));
    options
}

pub(crate) fn can_prompt(non_interactive: bool) -> bool {
    !non_interactive && stdin().is_terminal() && stderr().is_terminal()
}

pub(crate) fn select_one(
    explicit: Option<Harness>,
    non_interactive: bool,
) -> Result<Option<Harness>, AppError> {
    if explicit.is_some() {
        return Ok(explicit);
    }
    if !can_prompt(non_interactive) {
        return Err(AppError::HarnessSelectionRequired);
    }
    let options = ordered_options(Harness::installed);
    let labels: Vec<_> = options.iter().map(|option| option.label()).collect();
    let selected = Select::new()
        .with_prompt("Select a harness")
        .items(&labels)
        .default(0)
        .report(false)
        .interact_opt()
        .map_err(|_| AppError::HarnessPrompt)?;
    Ok(selected.map(|index| options[index].harness))
}

pub(crate) fn select_many(
    explicit: Vec<Harness>,
    non_interactive: bool,
) -> Result<Vec<Harness>, AppError> {
    if !explicit.is_empty() {
        let mut selected = Vec::new();
        for harness in explicit {
            if !selected.contains(&harness) {
                selected.push(harness);
            }
        }
        return Ok(selected);
    }
    if !can_prompt(non_interactive) {
        return Err(AppError::HarnessSelectionRequired);
    }
    let options = ordered_options(Harness::installed);
    let labels: Vec<_> = options.iter().map(|option| option.label()).collect();
    let selected = MultiSelect::new()
        .with_prompt("Select harnesses to configure")
        .items(&labels)
        .report(false)
        .interact_opt()
        .map_err(|_| AppError::HarnessPrompt)?;
    Ok(selected
        .unwrap_or_default()
        .into_iter()
        .map(|index| options[index].harness)
        .collect())
}

fn executable_on_path(name: &str) -> bool {
    let Some(path) = env::var_os("PATH") else {
        return false;
    };
    env::split_paths(&path).any(|dir| {
        #[cfg(windows)]
        {
            [".exe", ".cmd", ".bat", ".com"]
                .iter()
                .any(|extension| is_executable(&dir.join(format!("{name}{extension}"))))
        }
        #[cfg(not(windows))]
        {
            is_executable(&dir.join(name))
        }
    })
}

fn is_executable(path: &Path) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installed_harnesses_sort_first_then_every_group_is_alphabetical() {
        for (claude, codex, expected) in [
            (false, false, ["Claude Code", "Codex"]),
            (true, true, ["Claude Code (installed)", "Codex (installed)"]),
            (true, false, ["Claude Code (installed)", "Codex"]),
            (false, true, ["Codex (installed)", "Claude Code"]),
        ] {
            let labels: Vec<_> = ordered_options(|harness| match harness {
                Harness::Claude => claude,
                Harness::Codex => codex,
            })
            .into_iter()
            .map(OptionItem::label)
            .collect();
            assert_eq!(labels, expected);
        }
    }

    #[test]
    fn repeated_explicit_harnesses_are_deduplicated_in_order() {
        assert_eq!(
            select_many(vec![Harness::Codex, Harness::Claude, Harness::Codex], true).unwrap(),
            vec![Harness::Codex, Harness::Claude]
        );
    }
}
