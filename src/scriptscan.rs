//! Read shell and Python scripts for their interface without executing them.

use crate::model::{Command, Opt};
use crate::parse::{infer_kind, parse_help};
use crate::re;
use crate::util::*;
use std::path::Path;

pub fn scan(path: &Path, name: &str) -> Option<Command> {
    let bytes = std::fs::read(path).ok()?;
    let src = String::from_utf8_lossy(&bytes[..bytes.len().min(500_000)]).into_owned();
    let first = src.split('\n').next().unwrap_or("");
    if first.contains("python") {
        return scan_python(&src, name);
    }
    if is_match(re!(r"\b(ba|z|k|da)?sh\b"), first) {
        return Some(scan_shell(&src, name));
    }
    None
}

// ---------------------------------------------------------------- shell

fn expanded_width(s: &str) -> usize {
    let mut w = 0;
    for c in s.chars() {
        if c == '\t' {
            w = (w / 8 + 1) * 8;
        } else {
            w += 1;
        }
    }
    w
}

fn usage_blocks(src: &str, name: &str) -> String {
    let mut texts: Vec<String> = vec![];
    let self_line = fancy_regex::Regex::new(&format!(r"(?m)^\s+{}\s", fancy_regex::escape(name))).ok();
    for m in find_all(re!(r#"(?sm)<<-?\s*['"]?(\w+)['"]?[^\n]*\n(.*?)\n\s*\1\s*$"#), src) {
        let body = group(&m, 2);
        let start = m.get(0).unwrap().start();
        let mut b = start.saturating_sub(120);
        while !src.is_char_boundary(b) {
            b -= 1;
        }
        let before = &src[b..start];
        let self_lines = self_line.as_ref().map(|r| find_all(r, body).len()).unwrap_or(0);
        if is_match(re!(r"(?i)usage|options|commands"), body)
            || is_match(re!(r"(usage|help)\w*\s*\(\)"), before)
            || self_lines >= 2
        {
            texts.push(body.to_string());
        }
    }
    for m in find_all(re!(r#"(?s)(?:echo|printf)\s+(?:-e\s+)?(['"])((?:(?!\1).)*[Uu]sage(?:(?!\1).)*)\1"#), src) {
        texts.push(group(&m, 2).replace("\\n", "\n"));
    }
    texts.join("\n\n")
}

fn case_arms(src: &str, start: usize, base_indent: usize) -> Vec<(String, String)> {
    let mut arms = vec![];
    let mut depth = 0;
    let mut arm_indent: Option<usize> = None;
    for ln in src[start..].split('\n').skip(1) {
        let s = ln.trim();
        if is_match(re!(r"^case\b"), s) {
            depth += 1;
        }
        if is_match(re!(r"^esac\b"), s) {
            if depth == 0 {
                break;
            }
            depth -= 1;
            continue;
        }
        if depth > 0 {
            continue;
        }
        let Some(m) = find(re!(r#"^([ \t]*)\(?([\w|.*'"-]+(?:\s*\|\s*[\w.*'"-]+)*)\)(.*)$"#), ln) else { continue };
        let ind = expanded_width(group(&m, 1));
        if ind <= base_indent {
            continue;
        }
        let ai = *arm_indent.get_or_insert(ind);
        if ind != ai {
            continue;
        }
        let comment = find(re!(r"(?:^|\s)#\s*(.+)$"), group(&m, 3)).map(|c| group(&c, 1).to_string()).unwrap_or_default();
        arms.push((group(&m, 2).to_string(), comment));
    }
    arms
}

pub fn scan_shell(src: &str, name: &str) -> Command {
    let mut cmd = Command::with_source(name, "script");
    let usage = usage_blocks(src, name);
    if !usage.is_empty() {
        cmd.merge(parse_help(&usage, name, "script"));
    }
    for m in find_all(re!(r#"getopts\s+['"]?:?([A-Za-z0-9:]+)['"]?"#), src) {
        let spec: Vec<char> = group(&m, 1).chars().collect();
        for (i, ch) in spec.iter().enumerate() {
            if *ch != ':' && cmd.option(&format!("-{ch}")).is_none() {
                let takes = i + 1 < spec.len() && spec[i + 1] == ':';
                cmd.options.push(Opt { shorts: vec![ch.to_string()], arg: if takes { Some("ARG".into()) } else { None }, ..Default::default() });
            }
        }
    }
    // The dispatcher is the outermost case on $1 / $cmd / $command / $action.
    let mut dispatch: Vec<(usize, usize)> = find_all(re!(r#"(?m)^(?P<ind>[ \t]*)case\s+"?\$\{?(?P<var>[\w@#:-]+)\}?"?\s+in\b"#), src)
        .iter()
        .filter(|m| ["1", "cmd", "command", "action", "sub", "subcmd", "verb", "op"].contains(&named(m, "var").split(':').next().unwrap_or("")))
        .map(|m| (expanded_width(named(m, "ind")), m.get(0).unwrap().start()))
        .collect();
    dispatch.sort_by_key(|d| d.0); // stable, like Python's sort
    if let Some(&(top, _)) = dispatch.first() {
        for (_, start) in dispatch.iter().filter(|d| d.0 == top) {
            for (pattern, comment) in case_arms(src, *start, top) {
                for alt in pattern.split('|').map(|p| p.trim().trim_matches(|c| c == '\'' || c == '"')) {
                    if alt.starts_with('-') {
                        if is_match(re!(r"^(?:--?[\w-]+)$"), alt) && cmd.option(alt).is_none() {
                            let mut o = Opt::default();
                            if let Some(l) = alt.strip_prefix("--") {
                                o.longs.push(l.into());
                            } else {
                                o.shorts.push(alt.trim_start_matches('-').into());
                            }
                            cmd.options.push(o);
                        }
                    } else if is_match(re!(r"^(?:[A-Za-z0-9][\w.-]*)$"), alt) && !cmd.subcommands.contains_key(alt) {
                        let mut sub = Command::with_source(alt, "script");
                        sub.desc = first_sentence(&comment, 90);
                        cmd.subcommands.insert(alt.to_string(), sub);
                    }
                }
            }
        }
    }
    if !cmd.subcommands.is_empty() && !is_match(re!(r"\b(FILE|PATH)\b|<file"), &usage) {
        cmd.takes_files = false;
    }
    cmd
}

// ---------------------------------------------------------------- python
//
// No Python parser here: a small tokenizer finds `X.add_argument(...)` / `X.add_parser(...)`
// calls, splits their arguments at top level, and reads string / list literals.

#[derive(Debug, Clone)]
enum Lit {
    Str(String),
    List(Vec<Option<String>>),
    Other(String),
}

/// Parse a Python string literal starting at `s[i]` (handles prefixes, '…', "…", triple quotes).
fn read_string(s: &[char], mut i: usize) -> Option<(String, usize)> {
    while i < s.len() && "rRbBuUfF".contains(s[i]) {
        i += 1;
    }
    let q = *s.get(i)?;
    if q != '\'' && q != '"' {
        return None;
    }
    let triple = i + 2 < s.len() && s[i + 1] == q && s[i + 2] == q;
    let mut j = if triple { i + 3 } else { i + 1 };
    let mut out = String::new();
    while j < s.len() {
        if s[j] == '\\' && j + 1 < s.len() {
            let n = s[j + 1];
            out.push(match n {
                'n' => '\n',
                't' => '\t',
                _ => n,
            });
            j += 2;
            continue;
        }
        if triple {
            if j + 2 < s.len() && s[j] == q && s[j + 1] == q && s[j + 2] == q {
                return Some((out, j + 3));
            }
        } else if s[j] == q {
            return Some((out, j + 1));
        }
        out.push(s[j]);
        j += 1;
    }
    None
}

/// Split the argument list of a call whose '(' is at `open`; returns (pieces, index after ')').
fn call_args(s: &[char], open: usize) -> (Vec<String>, usize) {
    let mut depth = 0;
    let mut pieces = vec![];
    let mut cur = String::new();
    let mut i = open;
    while i < s.len() {
        let c = s[i];
        if c == '\'' || c == '"' {
            let start = i;
            let mut k = i;
            while k > 0 && "rRbBuUfF".contains(s[k - 1]) {
                k -= 1;
            }
            let _ = k;
            if let Some((_, end)) = read_string(s, i) {
                cur.extend(&s[start..end]);
                i = end;
                continue;
            }
        }
        match c {
            '(' | '[' | '{' => {
                depth += 1;
                if depth > 1 {
                    cur.push(c);
                }
            }
            ')' | ']' | '}' => {
                depth -= 1;
                if depth == 0 {
                    if !cur.trim().is_empty() {
                        pieces.push(cur.trim().to_string());
                    }
                    return (pieces, i + 1);
                }
                cur.push(c);
            }
            ',' if depth == 1 => {
                pieces.push(cur.trim().to_string());
                cur.clear();
            }
            '#' if depth >= 1 => {
                while i < s.len() && s[i] != '\n' {
                    i += 1;
                }
                continue;
            }
            _ => cur.push(c),
        }
        i += 1;
    }
    (pieces, s.len())
}

fn literal(piece: &str) -> Lit {
    let c: Vec<char> = piece.trim().chars().collect();
    if c.is_empty() {
        return Lit::Other(String::new());
    }
    if c[0] == '[' || c[0] == '(' {
        let (items, _) = call_args(&c, 0);
        return Lit::List(items.iter().map(|it| match literal(it) {
            Lit::Str(s) => Some(s),
            Lit::Other(o) if !o.is_empty() => Some(o),
            _ => None,
        }).collect());
    }
    // adjacent string literals concatenate
    let mut i = 0;
    let mut out = String::new();
    let mut any = false;
    while i < c.len() {
        if c[i].is_whitespace() {
            i += 1;
            continue;
        }
        match read_string(&c, i) {
            Some((s, end)) => {
                out += &s;
                any = true;
                i = end;
            }
            None => return Lit::Other(piece.trim().to_string()),
        }
    }
    if any { Lit::Str(out) } else { Lit::Other(piece.trim().to_string()) }
}

fn kwarg(piece: &str) -> Option<(String, Lit)> {
    let m = find(re!(r"^([A-Za-z_]\w*)\s*=(?!=)\s*([\s\S]*)$"), piece)?;
    Some((group(&m, 1).to_string(), literal(group(&m, 2))))
}

struct Call {
    target: String,
    method: String,
    assigned_to: Option<String>,
    pieces: Vec<String>,
}

fn find_calls(src: &str) -> Vec<Call> {
    let chars: Vec<char> = src.chars().collect();
    let text: String = chars.iter().collect();
    let mut calls = vec![];
    // char-index based scan over the regex matches
    for m in find_all(re!(r"(?m)(?:^[ \t]*([A-Za-z_]\w*)\s*=\s*)?\b([A-Za-z_][\w.]*)\.(add_argument|add_parser)\s*\("), &text) {
        let open_byte = m.get(0).unwrap().end() - 1;
        let open = text[..open_byte].chars().count();
        let (pieces, _) = call_args(&chars, open);
        let target = group(&m, 2).rsplit('.').next().unwrap_or("").to_string();
        calls.push(Call {
            target,
            method: group(&m, 3).to_string(),
            assigned_to: m.get(1).map(|g| g.as_str().to_string()),
            pieces,
        });
    }
    calls
}

pub fn scan_python(src: &str, name: &str) -> Option<Command> {
    let mut cmd = Command::with_source(name, "script");
    let calls = find_calls(src);
    let mut parsers: Vec<(String, String)> = vec![]; // variable -> subcommand name

    let parser_entry = |c: &Call| -> Option<(String, String)> {
        let first = c.pieces.first()?;
        let Lit::Str(sub) = literal(first) else { return None };
        let mut help = String::new();
        for p in &c.pieces[1..] {
            if let Some((k, Lit::Str(v))) = kwarg(p) {
                if k == "help" {
                    help = v;
                }
            }
        }
        Some((sub, help))
    };

    // pass 1: assigned add_parser calls (Python: ast.Assign of a Call)
    for c in calls.iter().filter(|c| c.method == "add_parser" && c.assigned_to.is_some()) {
        if let Some((sub, help)) = parser_entry(c) {
            let mut node = Command::with_source(&sub, "script");
            node.desc = first_sentence(&help, 90);
            cmd.subcommands.insert(sub.clone(), node);
            parsers.push((c.assigned_to.clone().unwrap(), sub));
        }
    }
    // pass 2: every call in source order
    for c in &calls {
        if c.method == "add_parser" {
            if let Some((sub, help)) = parser_entry(c) {
                if !cmd.subcommands.contains_key(&sub) {
                    let mut node = Command::with_source(&sub, "script");
                    node.desc = first_sentence(&help, 90);
                    cmd.subcommands.insert(sub, node);
                }
            }
            continue;
        }
        let flags: Vec<String> = c
            .pieces
            .iter()
            .filter(|p| kwarg(p).is_none())
            .filter_map(|p| match literal(p) {
                Lit::Str(s) if s.starts_with('-') => Some(s),
                _ => None,
            })
            .collect();
        if flags.is_empty() {
            continue;
        }
        let mut o = Opt::default();
        for fl in &flags {
            if let Some(l) = fl.strip_prefix("--") {
                o.longs.push(l.into());
            } else if char_len(fl) == 2 {
                o.shorts.push(fl[1..].into());
            } else {
                o.olds.push(fl[1..].into());
            }
        }
        let mut broke = false;
        for p in &c.pieces {
            let Some((k, v)) = kwarg(p) else { continue };
            match (k.as_str(), v) {
                ("help", Lit::Str(h)) => o.desc = first_sentence(&h, 90),
                ("choices", Lit::List(items)) => o.values = items.into_iter().flatten().map(|v| (v, String::new())).collect(),
                ("metavar", Lit::Str(mv)) => o.arg = Some(mv),
                ("action", Lit::Str(a)) if ["store_true", "store_false", "count", "help", "version"].contains(&a.as_str()) => {
                    o.arg = None;
                    broke = true;
                    break;
                }
                _ => {}
            }
        }
        if !broke && o.arg.is_none() {
            o.arg = Some(o.longs.first().map(|l| l.to_uppercase()).unwrap_or_else(|| "ARG".into()));
        }
        o.kind = infer_kind(&o);
        let owner_sub = parsers.iter().find(|(var, _)| *var == c.target).map(|(_, s)| s.clone());
        let owner: &mut Command = match owner_sub {
            Some(s) => cmd.subcommands.get_mut(&s).unwrap(),
            None => &mut cmd,
        };
        let first = o.flags()[0].clone();
        if owner.option(&first).is_none() {
            owner.options.push(o);
        }
    }
    for m in find_all(re!(r#"sys\.argv\[1\]\s*==\s*['"]([\w.-]+)['"]"#), src) {
        let s = group(&m, 1);
        if !cmd.subcommands.contains_key(s) {
            cmd.subcommands.insert(s.to_string(), Command::with_source(s, "script"));
        }
    }
    if cmd.options.is_empty() && cmd.subcommands.is_empty() {
        return None;
    }
    Some(cmd)
}
