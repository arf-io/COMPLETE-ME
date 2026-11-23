//! A literal environment change file. Values never pass through a shell parser.
use std::io::{self, Read, Write};

type Changes = indexmap::IndexMap<String, Option<String>>;
const MAX_INPUT: u64 = 1024 * 1024;

fn name_ok(name: &str) -> bool {
    crate::sh::ast::valid_name(name)
        && !name.starts_with("__")
        && !name.starts_with("fish_")
        && !matches!(
            name,
            "PWD" | "SHLVL" | "_" | "argv" | "status" | "pipestatus" | "history"
                | "version" | "FISH_VERSION" | "hostname" | "umask"
        )
}

fn value(text: &str) -> Option<String> {
    let text = text.trim();
    if let Some(quote @ ('\'' | '"')) = text.chars().next() {
        let mut chars = text[1..].char_indices();
        let mut value = String::new();
        while let Some((index, ch)) = chars.next() {
            if ch == quote {
                let tail = text[index + 2..].trim_start();
                return (tail.is_empty() || tail.starts_with('#')).then_some(value);
            }
            if quote == '"' && ch == '\\' {
                value.push(match chars.next()?.1 {
                    'n' => '\n',
                    'r' => '\r',
                    't' => '\t',
                    ch @ ('\\' | '"' | '$') => ch,
                    _ => return None,
                });
            } else {
                value.push(ch);
            }
        }
        return None;
    }
    let comment = text.char_indices().find_map(|(index, ch)| {
        (ch == '#' && (index == 0 || text[..index].ends_with(char::is_whitespace))).then_some(index)
    });
    let text = text[..comment.unwrap_or(text.len())].trim_end();
    // Bare values use a small literal alphabet. Quotes can hold arbitrary text.
    text.chars()
        .all(|ch| ch.is_alphanumeric() || "_-./:@%+,=".contains(ch))
        .then(|| text.to_owned())
}

fn parse(input: &str) -> Result<Changes, usize> {
    let mut changes = Changes::new();
    for (index, text) in input.lines().enumerate() {
        let text = text.trim();
        if text.is_empty() || text.starts_with('#') {
            continue;
        }
        let line = index + 1;
        let (name, value) = if let Some(name) = text.strip_prefix("unset").filter(|s| s.starts_with(char::is_whitespace)) {
            (name.split_once('#').map_or(name, |(name, _)| name).trim(), None)
        } else {
            let assignment = text
                .strip_prefix("export")
                .filter(|s| s.starts_with(char::is_whitespace))
                .unwrap_or(text);
            let (name, text) = assignment.split_once('=').ok_or(line)?;
            (name.trim(), Some(value(text).filter(|s| !s.contains('\0')).ok_or(line)?))
        };
        if !name_ok(name) {
            return Err(line);
        }
        changes.insert(name.to_owned(), value);
    }
    Ok(changes)
}

fn input(source: &str) -> Option<String> {
    let reader: Box<dyn Read> = if source == "-" {
        Box::new(io::stdin())
    } else {
        Box::new(crate::util::open_regular(source).ok()?)
    };
    let mut text = String::new();
    reader.take(MAX_INPUT + 1).read_to_string(&mut text).ok()?;
    (text.len() as u64 <= MAX_INPUT).then_some(text)
}

pub(crate) fn run(args: &[String]) -> i32 {
    let (mode, source) = match args {
        [mode, source] if matches!(mode.as_str(), "diff" | "_plan") => (mode.as_str(), source),
        [mode, ..] if matches!(mode.as_str(), "apply" | "undo") => {
            eprintln!("complete-me env {mode} requires the loaded fish integration");
            return 2;
        }
        _ => {
            eprintln!("usage: complete-me env diff FILE|- | apply [--yes] FILE|- | undo");
            return 2;
        }
    };
    let Some(text) = input(source) else {
        eprintln!("complete-me env: expected a UTF-8 input of at most 1 MiB");
        return 1;
    };
    let changes = match parse(&text) {
        Ok(changes) => changes,
        Err(line) => {
            eprintln!("complete-me env: invalid or protected assignment on line {line}");
            return 1;
        }
    };
    let mut out = io::stdout().lock();
    let mut changed = false;
    for (name, value) in changes {
        let previous = std::env::var_os(&name);
        let verb = match (previous.as_deref(), value.as_deref()) {
            (None, None) => continue,
            (None, Some(_)) => "add",
            (Some(_), None) => "remove",
            (Some(old), Some(new)) if old == new => continue,
            (Some(_), Some(_)) => "change",
        };
        changed = true;
        let result = if mode == "_plan" {
            write!(out, "{verb}\0{name}\0{}\0", value.unwrap_or_default())
        } else {
            writeln!(out, "{verb} {name}")
        };
        if result.is_err() {
            return 1;
        }
    }
    if !changed && mode == "diff" {
        return i32::from(writeln!(out, "No environment changes.").is_err());
    }
    if mode == "_plan" {
        return i32::from(write!(out, "ready\0").is_err());
    }
    0
}
