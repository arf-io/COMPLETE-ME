//! Locate executable candidates in PATH and add the caller shell's resolution.
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::util;

fn visible(text: &str) -> String {
    let mut output = String::new();
    for ch in text.chars() {
        if ch.is_control() { output.extend(ch.escape_default()); } else { output.push(ch); }
    }
    output
}

pub(crate) fn run(args: &[String]) -> i32 {
    let (name, shell_kind, shell_file) = match args {
        [mode, name] if mode == "why" => (name, "", ""),
        [mode, name, kind, file] if mode == "_why" => (name, kind.as_str(), file.as_str()),
        _ => {
            eprintln!("usage: complete-me path why COMMAND");
            return 2;
        }
    };
    if name.is_empty() || name.starts_with('-') || name.chars().any(char::is_control) {
        eprintln!("complete-me path why: expected a command name");
        return 2;
    }

    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let absolute = |path: &Path| cwd.join(path);
    let path = std::env::var_os("PATH");
    let dirs = path.as_ref().map(|p| std::env::split_paths(p).collect::<Vec<_>>()).unwrap_or_default();
    let count_duplicates = !name.contains('/');
    let candidates: Vec<_> = if count_duplicates {
        dirs.iter().enumerate().filter_map(|(index, dir)| {
            let candidate = absolute(dir).join(name);
            util::is_executable(&candidate).then_some((Some(index + 1), candidate))
        }).collect()
    } else {
        let explicit = absolute(Path::new(name));
        util::is_executable(&explicit).then_some((None, explicit)).into_iter().collect()
    };
    let selected_path = (!shell_file.is_empty()).then(|| absolute(Path::new(shell_file)))
        .or_else(|| candidates.first().map(|(_, path)| path.clone()));
    let selected_exists = match shell_kind {
        kind @ ("alias" | "function" | "builtin" | "reserved") => {
            println!("Selected: {kind} {}", visible(name));
            true
        }
        _ if selected_path.is_some() => {
            let file = selected_path.as_ref().unwrap();
            println!("Selected: {}", visible(&file.to_string_lossy()));
            true
        }
        _ => {
            println!("{} was not found.", visible(name));
            false
        }
    };

    let mut selected_row = false;
    for (index, candidate) in candidates {
        let Some(index) = index else { continue };
        let label = if !selected_row && !matches!(shell_kind, "alias" | "function" | "builtin" | "reserved")
            && selected_path.as_ref() == Some(&candidate)
        {
            selected_row = true;
            "selected"
        } else {
            "shadowed"
        };
        println!("  PATH[{index}] {label}: {}", visible(&candidate.to_string_lossy()));
    }
    if count_duplicates {
        let mut seen = HashMap::new();
        for (index, dir) in dirs.into_iter().enumerate() {
            let dir = absolute(&dir);
            if let Some(first) = seen.get(&dir) {
                println!("  PATH[{}] duplicates PATH[{first}]: {}", index + 1, visible(&dir.to_string_lossy()));
            } else {
                seen.insert(dir, index + 1);
            }
        }
    }
    i32::from(!selected_exists)
}
