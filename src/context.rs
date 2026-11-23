//! Resolve the command path and argument under the cursor from a generated grammar.
use crate::{extras, util::Args};
use std::collections::HashMap;

/// Find the option taking a value in a long option or a short option cluster.
/// Return the flag and the offset at which an attached value would start.
pub(crate) fn value_option(word: &str, takes_value: impl Fn(&str) -> bool) -> Option<(&str, usize)> {
    if word.starts_with("--") {
        let flag = word.split('=').next()?;
        return takes_value(flag).then_some((flag, flag.len() + usize::from(flag.len() < word.len())));
    }
    // Some CLIs have single-dash long options as well as short clusters.
    if word.starts_with('-') && takes_value(word) {
        return Some((word, word.len()));
    }
    let short = word.strip_prefix('-')?;
    for (i, c) in short.char_indices() {
        let end = i + c.len_utf8() + 1;
        let flag = &word[i + 1..end];
        if takes_value(&format!("-{flag}")) {
            return Some((flag, end));
        }
    }
    None
}

#[derive(Debug, Default, PartialEq)]
struct Context<'a> {
    path: String,
    flag: &'a str,
    flag_path: &'a str,
    index: usize,
    offset: usize,
    subcommands: bool,
    options: bool,
}

struct Flag<'a> {
    name: &'a str,
    path: &'a str,
    kind: &'a str,
}

fn resolve<'a>(grammar: &'a [String], words: &[String], current: &str) -> Context<'a> {
    let mut ctx = Context {
        subcommands: true,
        options: true,
        ..Context::default()
    };
    let mut pending = None;
    // Canonical paths resolve before aliases. The nearest declaration wins when
    // a parent and a child give the same option different meanings.
    let mut commands: HashMap<&str, HashMap<&str, &str>> = HashMap::new();
    let mut flags: HashMap<&str, HashMap<&str, Flag<'_>>> = HashMap::new();
    for row in grammar {
        let fields: Vec<_> = row.split('|').collect();
        match fields.as_slice() {
            ["C", path, word, name] => {
                commands.entry(path).or_default().insert(word, name);
            }
            [kind @ ("R" | "O" | "F"), path, flag] => {
                flags
                    .entry(path)
                    .or_default()
                    .insert(flag, Flag { name: flag, path, kind });
            }
            _ => {}
        }
    }
    let lookup = |mut path: &str, flag: &str| loop {
        if let Some(value) = flags.get(path).and_then(|node| node.get(flag)) {
            break Some(value);
        }
        if path.is_empty() {
            break None;
        }
        path = path.rsplit_once(' ').map_or("", |(parent, _)| parent);
    };
    for (i, word) in words.iter().enumerate() {
        if pending.take().is_some() {
            continue;
        }
        if ctx.options && word == "--" {
            ctx.subcommands = false;
            ctx.options = false;
            continue;
        }
        if ctx.options && word.starts_with('-') && word != "-" {
            if let Some((flag, offset)) =
                value_option(word, |f| lookup(&ctx.path, f).is_some_and(|option| option.kind != "F"))
            {
                let spelling = if flag.starts_with('-') {
                    flag.to_string()
                } else {
                    format!("-{flag}")
                };
                if offset == word.len() && !word.ends_with('=') {
                    pending = lookup(&ctx.path, &spelling).filter(|option| option.kind == "R");
                }
            }
            continue;
        }
        if ctx.subcommands {
            if let Some(name) = commands
                .get(ctx.path.as_str())
                .and_then(|node| node.get(word.as_str()))
                .copied()
            {
                if !ctx.path.is_empty() {
                    ctx.path.push(' ');
                }
                ctx.path.push_str(name);
                ctx.index = i + 1;
                continue;
            }
            ctx.subcommands = false;
        }
    }
    if let Some(flag) = pending {
        ctx.flag = flag.name;
        ctx.flag_path = flag.path;
    } else if ctx.options {
        if let Some((flag, offset)) = value_option(current, |f| {
            lookup(&ctx.path, f).is_some_and(|option| option.kind != "F")
        })
        .filter(|(_, offset)| *offset < current.len() || current.starts_with("--") && current.ends_with('='))
        {
            let spelling = if flag.starts_with('-') {
                flag.to_string()
            } else {
                format!("-{flag}")
            };
            if let Some(flag) = lookup(&ctx.path, &spelling) {
                ctx.flag = flag.name;
                ctx.flag_path = flag.path;
                ctx.offset = offset;
            }
        }
    }
    ctx
}

pub(crate) fn run(args: &[String]) -> i32 {
    let a = Args::parse(args, &["--line", "--current", "--breaks"]);
    let Some(sep) = a.rest.iter().position(|s| s == "--") else {
        return 2;
    };
    let grammar = &a.rest[..sep];
    let words = &a.rest[sep + 1..];
    let from_line = a.value("--line").map(|line| extras::split_line(&line));
    let current = a.value("--current").unwrap_or_default();
    let (words, current) = from_line.as_ref().map_or((words, current.as_str()), |line| {
        (line.words.get(1..).unwrap_or_default(), line.cur.as_str())
    });
    let ctx = resolve(grammar, words, current);
    // Shells read one record with no evaluation. Valid grammar names exclude |.
    let insert = if ctx.offset > 0 {
        let prefix = &current[..ctx.offset];
        let breaks = a.value("--breaks").unwrap_or_default();
        if breaks.contains('=') && prefix.ends_with('=') {
            ""
        } else {
            prefix
        }
    } else {
        ""
    };
    let value = &current[ctx.offset..];
    // Bash uses its own current word when a tab/newline would break the protocol.
    let value = if value.contains(['|', '\n', '\r']) { "" } else { value };
    println!(
        "{}|{}|{}|{}|{}|{}|{}|{}|{}",
        ctx.path,
        ctx.flag,
        ctx.index,
        ctx.offset,
        u8::from(ctx.subcommands),
        u8::from(ctx.options),
        ctx.flag_path,
        insert,
        value
    );
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).into()).collect()
    }

    #[test]
    fn value_slots_use_the_nearest_declaration() {
        let grammar = strings(&[
            "C||g|group",
            "C|group|b|build",
            "R||-C",
            "R||-t",
            "F|group|-t",
            "O||--theme",
            "R|group build|--mode",
        ]);
        let words = strings(&["-qC", "g", "--theme", "g", "-t", "b"]);
        let ctx = resolve(&grammar, &words, "--mode=au");
        assert_eq!(
            (ctx.path.as_str(), ctx.flag, ctx.index, ctx.offset),
            ("group build", "--mode", 6, 7)
        );
        let ctx = resolve(&grammar, &strings(&["g", "b", "-C"]), "some dir");
        assert_eq!((ctx.path.as_str(), ctx.flag, ctx.offset), ("group build", "-C", 0));
        let ctx = resolve(&grammar, &strings(&["g", "--", "b"]), "--mode=");
        assert_eq!(
            (ctx.path.as_str(), ctx.flag, ctx.options, ctx.subcommands),
            ("group", "", false, false)
        );
    }

    #[test]
    fn stop_guessing_subcommands_after_an_operand() {
        let grammar = strings(&["C||g|group", "C|group|b|build", "R|group|--mode"]);
        let ctx = resolve(&grammar, &strings(&["g", "an operand", "b", "--mode"]), "test");
        assert_eq!(
            (ctx.path.as_str(), ctx.flag, ctx.index, ctx.subcommands),
            ("group", "--mode", 1, false)
        );
    }
}
