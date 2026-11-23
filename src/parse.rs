//! Turn --help output and man pages into a Command model.
//! Pure text processing: no subprocesses, no shell.

use crate::model::{Command, Opt};
use crate::re;
use crate::util::*;

pub fn clean(text: &str) -> String {
    let t = replace_all(re!(r"\x1b\[[0-9;?]*[A-Za-z]|\x1b\][^\x07]*\x07"), text, "");
    let t = replace_all(re!(r".\x08"), &t, "");
    t.replace('\t', "        ").replace('\r', "")
}

pub fn indent_of(line: &str) -> usize {
    line.len() - line.trim_start_matches(' ').len()
}

// ---------------------------------------------------------------- sections

const NON_OPTION_SECTIONS: &[&str] = &[
    "example", "see also", "environment", "files", "exit status", "notes", "bugs", "author", "copyright",
    "reporting", "history", "standards",
];
const NOT_COMMAND_WORDS: &[&str] = &["option", "flag", "switch", "argument"];

pub fn header_of(line: &str) -> Option<String> {
    let stripped = line.trim_end();
    let t = stripped.trim();
    if t.is_empty() {
        return None;
    }
    let ind = indent_of(stripped);
    if ind <= 1 && t.ends_with(':') && char_len(t) < 70 && !t.starts_with('-') {
        return Some(t[..t.len() - 1].to_lowercase());
    }
    if ind == 0 && is_match(re!(r"^(?:[A-Z][A-Z0-9 /_&-]{2,})$"), t) {
        return Some(t.to_lowercase());
    }
    None
}

fn is_command_header(h: &str) -> bool {
    is_match(re!(r"\b(sub)?commands?\b"), h) && !NOT_COMMAND_WORDS.iter().any(|w| h.contains(w))
}

// ---------------------------------------------------------------- options

/// Match `r` exactly at byte offset `pos` of `s` (Python's pattern.match(s, pos)).
fn match_at<'a>(r: &fancy_regex::Regex, s: &'a str, pos: usize) -> Option<fancy_regex::Captures<'a>> {
    let c = r.captures_from_pos(s, pos).ok().flatten()?;
    if c.get(0).map(|m| m.start()) == Some(pos) {
        Some(c)
    } else {
        None
    }
}

fn arg_forms() -> [&'static fancy_regex::Regex; 6] {
    [
        re!(r"\[=(?P<arg>[^\]]+)\]"),
        re!(r"=(?P<arg><[^>]+>|[^\s,\]]+)"),
        re!(r"\[(?P<arg><[^>]+>|[A-Z][\w-]*)\]"),
        re!(r"(?P<arg><[^>]+>(?:\.\.\.)?)"),
        re!(r"[ ](?! )(?P<arg><[^>]+>(?:\.\.\.)?|\{[^}]+\}|\[[^\]]+\]|[A-Z][A-Z0-9_|:-]*(?:\.\.\.)?)"),
        re!(r"[ ](?! )(?P<arg>[a-z][\w-]*)(?=\s*,\s*-|\s{2,}|\s*$)"),
    ]
}

/// Return (Opt, rest_of_line) if the line begins an option entry.
pub fn parse_option_spec(line: &str) -> Option<(Opt, String)> {
    let body = line.trim_start_matches(' ');
    if !body.starts_with('-') || body.starts_with("---") {
        return None;
    }
    let mut pos = 0usize;
    let mut opt = Opt::default();
    while let Some(m) = match_at(re!(r"(--?[A-Za-z0-9?#][\w.#+?-]*)"), body, pos) {
        let whole = m.get(1).unwrap();
        let flag = whole.as_str().trim_end_matches('.');
        pos = whole.end();
        if body[pos..].starts_with("...") {
            pos += 3;
        }
        if flag == "-" || flag == "--" {
            return None;
        }
        if let Some(l) = flag.strip_prefix("--") {
            opt.longs.push(l.to_string());
        } else if char_len(flag) == 2 {
            opt.shorts.push(flag[1..].to_string());
        } else {
            opt.olds.push(flag[1..].to_string());
        }
        for form in arg_forms() {
            if let Some(a) = match_at(form, body, pos) {
                opt.arg = Some(named(&a, "arg").to_string());
                opt.arg_optional = body[pos..].starts_with("[=") || body[pos..].starts_with('[');
                pos = a.get(0).unwrap().end();
                break;
            }
        }
        if let Some(s) = match_at(re!(r"\s*,\s*|\s*\|\s*|\s+or\s+|[ ](?=-)"), body, pos) {
            let e = s.get(0).unwrap().end();
            if body[e..].starts_with('-') {
                pos = e;
                continue;
            }
        }
        break;
    }
    if opt.is_empty() {
        return None;
    }
    let rest = &body[pos..];
    if !rest.is_empty() && !is_match(re!(r"^(?:\s{2,}|\s+-\s+|\s*$)"), rest) {
        return None; // prose that merely starts with a dash
    }
    let inline = if rest.trim().is_empty() {
        String::new()
    } else {
        replace_all(re!(r"^\s*-?\s*"), rest, "").trim_end().to_string()
    };
    Some((opt, inline))
}

// ---------------------------------------------------------------- values

const QUOTED: &str = r#"['"`‘“]([\w.@+:-]{1,30})['"`’”]"#;

fn quoted_all(s: &str) -> Vec<String> {
    find_all(re!(QUOTED), s).iter().map(|c| group(c, 1).to_string()).collect()
}

fn dedupe(v: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    for x in v {
        if !out.contains(&x) {
            out.push(x);
        }
    }
    out
}

fn pairs_empty(v: Vec<String>) -> Vec<(String, String)> {
    v.into_iter().map(|x| (x, String::new())).collect()
}

fn is_upper(s: &str) -> bool {
    s.chars().any(|c| c.is_alphabetic()) && !s.chars().any(|c| c.is_lowercase())
}

fn strip_chars<'a>(s: &'a str, set: &str) -> &'a str {
    s.trim_matches(|c| set.contains(c))
}

pub fn extract_values(opt: &Opt, full: &str) -> Vec<(String, String)> {
    let arg = opt.arg.clone().unwrap_or_default();
    let inner = strip_chars(&arg, "<>[]{}");
    if (arg.starts_with('{') || arg.contains('|')) && !inner.is_empty() {
        let vals: Vec<String> = inner
            .split([',', '|'])
            .filter(|v| is_match(re!(r"^[\w.+@-]{1,30}$"), v))
            .map(String::from)
            .collect();
        if vals.len() >= 2 {
            return pairs_empty(vals);
        }
    }
    if let Some(m) =
        find(re!(r"(?i)(?:possible values|values are|one of the following)[^\n]*:\s*\n(?P<rest>(?:.*\n?){1,60})"), full)
    {
        let pairs: Vec<(String, String)> = find_all(re!(r"(?m)^\s*(?:-\s+)?([\w.+-]{1,30}):\s+(\S.*)$"), named(&m, "rest"))
            .iter()
            .map(|c| (group(c, 1).to_string(), first_sentence(group(c, 2), 60)))
            .collect();
        if pairs.len() >= 2 {
            return pairs;
        }
    }
    if let Some(m) = find(re!(r"\[possible values: ([^\]]+)\]"), full) {
        let vals = split_value_list(group(&m, 1));
        if !vals.is_empty() {
            return pairs_empty(vals);
        }
    }
    let phrase = re!(
        r"(?is)(?:possible values(?: for this \w+)?(?: are)?|valid (?:values|arguments|options)(?: are)?|accepted values(?: are)?|one of|must be(?: one of)?|may be(?: one of)?|can be(?: one of)?|choices(?: are)?|either|supported values(?: are)?)\s*:?\s*(?P<tail>.{0,300})"
    );
    for m in find_all(phrase, full) {
        let tail = named(&m, "tail");
        let sentence = match find(re!(r"(?<=[.;])\s"), tail) {
            Some(c) => &tail[..c.get(0).unwrap().start()],
            None => tail,
        };
        let quoted = quoted_all(sentence);
        if quoted.len() >= 2 {
            return pairs_empty(dedupe(quoted));
        }
        let vals = split_value_list(tail);
        if !vals.is_empty() {
            return pairs_empty(vals);
        }
    }
    if let Some(m) = find(re!(r#"\(((?:\s*["'][\w.-]+["'],?){3,})\s*\)"#), full) {
        return pairs_empty(dedupe(quoted_all(group(&m, 1))));
    }
    let pairs = dedupe(
        find_all(re!(r"\b([a-z][\w-]*) \((?:--?[\w-]+|default)\)"), full).iter().map(|c| group(c, 1).to_string()).collect(),
    );
    if pairs.len() >= 2 {
        return pairs_empty(pairs);
    }
    if let Some(m) = find(re!(r":\s*((?:[a-z][\w-]*,\s*){2,}[a-z][\w-]*)\b"), full) {
        if !inner.is_empty() && is_upper(inner) {
            let vals = split_value_list(group(&m, 1));
            if !vals.is_empty() {
                return pairs_empty(vals);
            }
        }
    }
    vec![]
}

/// GNU style: "The WHEN argument defaults to 'always' and can also be 'auto' or 'never'."
pub fn values_from_placeholder(placeholder: &str, text: &str) -> Vec<(String, String)> {
    let word = strip_chars(placeholder, "<>[]=.");
    if !is_match(re!(r"^(?:[A-Z][A-Z_]{2,})$"), word) {
        return vec![];
    }
    let pat = match fancy_regex::Regex::new(&format!(r"\b{word}\b (?:argument |value )?(?:is|can|may|must|defaults)")) {
        Ok(r) => r,
        Err(_) => return vec![],
    };
    let squashed = squash_ws(text);
    for sentence in re!(r"(?<=[.])\s+").split(&squashed).filter_map(|r| r.ok()) {
        if is_match(&pat, sentence) {
            let q = quoted_all(sentence);
            if q.len() >= 2 {
                return pairs_empty(dedupe(q));
            }
        }
    }
    vec![]
}

pub fn infer_kind(opt: &Opt) -> String {
    if !opt.values.is_empty() {
        return "enum".into();
    }
    let Some(arg) = &opt.arg else { return String::new() };
    let a = strip_chars(arg, "<>[]=.").replace('-', "_").to_lowercase();
    let kinds: [(&str, &fancy_regex::Regex); 8] = [
        ("dir", re!(r"\b(dir|directory|dirs|directories|folder)\b")),
        ("file", re!(r"\b(file|files|filename|path|paths|manifest|config|output|input|socket|log_file|script)\b|_file\b|\.\w+$")),
        ("user", re!(r"\b(user|username|login)\b")),
        ("group", re!(r"\bgroup\b")),
        ("host", re!(r"\b(host|hostname|address|addr)\b")),
        ("pid", re!(r"\bpid\b")),
        ("command", re!(r"\b(command|cmd|program)\b")),
        ("number", re!(r"\b(n|num|number|int|count|size|seconds|secs|ms|port|level|depth|bytes|lines|jobs|width)\b")),
    ];
    for (k, r) in kinds {
        if is_match(r, &a) {
            return k.into();
        }
    }
    String::new()
}

// ---------------------------------------------------------------- commands

const CMD_ARGS: &str = r"(?P<args>(?:\s+(?:<[^>]*>|\[[^\]]*\]|[A-Z][A-Z0-9_.|,-]*(?:\.\.\.|…)?|\{[^}]*\}))*)";

fn cmd_entry() -> &'static fancy_regex::Regex {
    re!(r"^(?P<ind>\s{1,10})(?P<names>[A-Za-z0-9][\w.:+-]*(?:\(\d\w*\))?\*?(?:\s*,\s*[A-Za-z0-9][\w.:+-]*)*)(?P<args>(?:\s+(?:<[^>]*>|\[[^\]]*\]|[A-Z][A-Z0-9_.|,-]*(?:\.\.\.|…)?|\{[^}]*\}))*)(?:\s{2,}|\s+-\s+)(?:-\s+)?(?P<desc>\S.*)$")
}
fn cmd_alone() -> &'static fancy_regex::Regex {
    re!(r"^(?P<ind>\s{1,10})(?P<names>[A-Za-z0-9][\w.:+-]*(?:\(\d\w*\))?(?:\s*,\s*[A-Za-z0-9][\w.:+-]*)*)(?P<args>(?:\s+(?:<[^>]*>|\[[^\]]*\]|[A-Z][A-Z0-9_.|,-]*(?:\.\.\.|…)?|\{[^}]*\}))*)\s*$")
}
#[allow(dead_code)]
const _ARGS_DOC: &str = CMD_ARGS;

fn normalise_cmd_name(raw: &str, parent: &str) -> String {
    let n = replace_all(re!(r"\(\d\w*\)$"), raw.trim_end_matches('*'), "");
    match n.strip_prefix(&format!("{parent}-")) {
        Some(s) => s.to_string(),
        None => n,
    }
}

fn usage_file() -> &'static fancy_regex::Regex {
    re!(r"(?i)\b(FILE|PATH|DIR|DIRECTORY|SOURCE|DEST|TARGET|FILENAME)S?\b|<(file|path|dir)[\w-]*>|\bfile\b")
}

fn valid_name(s: &str) -> bool {
    is_match(re!(r"^(?:\w[\w.:+-]*)$"), s)
}

fn upsert_sub<'a>(cmd: &'a mut Command, name: &str, source: &str) -> &'a mut Command {
    if !cmd.subcommands.contains_key(name) {
        cmd.subcommands.insert(name.to_string(), Command::with_source(name, source));
    }
    cmd.subcommands.get_mut(name).unwrap()
}

pub fn parse_help(text: &str, name: &str, source: &str) -> Command {
    let text = clean(text);
    let lines: Vec<&str> = text.split('\n').collect();
    let mut cmd = Command::with_source(name, source);
    let mut section = String::new();
    let mut in_cmds = false;
    let mut usage_lines: Vec<String> = vec![];
    let mut in_usage = false;
    let self_named_re = fancy_regex::Regex::new(&format!(r"^(\s+){}\s+(?=[A-Za-z0-9])", fancy_regex::escape(name))).ok();
    let mut i = 0usize;
    while i < lines.len() {
        let line = lines[i].trim_end();
        let h = header_of(line);
        if let Some(um) = find(re!(r"(?i)^\s*usage:\s*(.*)$"), line) {
            in_usage = true;
            usage_lines.push(group(&um, 1).to_string());
            i += 1;
            continue;
        }
        if in_usage {
            if !line.trim().is_empty() && h.is_none() {
                usage_lines.push(line.to_string());
                i += 1;
                continue;
            }
            in_usage = false;
        }
        if let Some(h) = &h {
            section = h.clone();
            in_cmds = is_command_header(h);
            if h == "synopsis" {
                in_usage = true;
            }
            i += 1;
            continue;
        }
        if line.trim().is_empty() {
            i += 1;
            continue;
        }
        if NON_OPTION_SECTIONS.iter().any(|s| section.starts_with(s)) {
            i += 1;
            continue;
        }

        if let Some((mut opt, inline)) = parse_option_spec(line) {
            let ind = indent_of(line);
            let mut desc_lines: Vec<String> = if inline.is_empty() { vec![] } else { vec![inline.clone()] };
            let mut desc_col = if inline.is_empty() { ind + 3 } else { char_len(line) - char_len(&inline) };
            let mut j = i + 1;
            while j < lines.len() {
                let nxt = lines[j].trim_end();
                if nxt.trim().is_empty() {
                    let mut k = j + 1;
                    while k < lines.len() && lines[k].trim().is_empty() {
                        k += 1;
                    }
                    if k < lines.len() && indent_of(lines[k]) > ind + 2 && header_of(lines[k]).is_none() {
                        desc_lines.push(String::new());
                        j = k;
                        continue;
                    }
                    break;
                }
                let nind = indent_of(nxt);
                if nind <= ind || header_of(nxt).is_some() {
                    break;
                }
                if nxt.trim_start().starts_with('-') && nind < desc_col && parse_option_spec(nxt).is_some() {
                    break;
                }
                if desc_lines.is_empty() {
                    desc_col = nind;
                }
                desc_lines.push(nxt.trim().to_string());
                j += 1;
            }
            let full = desc_lines.join("\n");
            opt.desc = first_sentence(full.split("\n\n").next().unwrap_or(""), 90);
            let tail = lines[i + 1..j].join("\n");
            let raw_full = if inline.is_empty() { tail } else { format!("{inline}\n{tail}") };
            opt.values = extract_values(&opt, &raw_full);
            if opt.values.is_empty() {
                if let Some(arg) = opt.arg.clone() {
                    opt.values = values_from_placeholder(&arg, &text);
                }
            }
            opt.kind = infer_kind(&opt);
            let first = opt.flags()[0].clone();
            match cmd.option_mut(&first) {
                None => cmd.options.push(opt),
                Some(existing) => {
                    if existing.desc.is_empty() {
                        existing.desc = opt.desc;
                    }
                }
            }
            i = j;
            continue;
        }

        if let Some(b) = find(re!(r"^\s+\{([\w,.-]+)\}\s*"), line) {
            if ["positional", "commands", "subcommands", "available"].iter().any(|p| section.starts_with(p)) {
                for n in group(&b, 1).split(',') {
                    upsert_sub(&mut cmd, n, source);
                }
                in_cmds = true;
                i += 1;
                continue;
            }
        }

        if let Some(sn) = self_named_re.as_ref().and_then(|r| find(r, line)) {
            let rebuilt = format!("{}{}", group(&sn, 1), &line[sn.get(0).unwrap().end()..]);
            if let Some(m) = find(cmd_entry(), &rebuilt) {
                let primary = normalise_cmd_name(named(&m, "names").split(',').next().unwrap_or("").trim(), name);
                if valid_name(&primary) {
                    let args = named(&m, "args").to_string();
                    let desc = named(&m, "desc").to_string();
                    let sub = upsert_sub(&mut cmd, &primary, source);
                    if sub.desc.is_empty() {
                        sub.desc = first_sentence(&desc, 90);
                    }
                    sub.takes_files =
                        is_match(usage_file(), &args) || is_match(re!(r"<[^>]*\.\w+>"), &args);
                    i += 1;
                    continue;
                }
            }
        }

        if !in_cmds && (section.is_empty() || section == "usage") {
            // GLib-style: a bare table of commands with no header. Needs 3 entries in a row.
            let mut run = 0;
            for k in i..(i + 12).min(lines.len()) {
                let alone_with_desc = is_match(cmd_alone(), lines[k])
                    && k + 1 < lines.len()
                    && indent_of(lines[k + 1]) > indent_of(lines[k]) + 8;
                if is_match(cmd_entry(), lines[k]) || alone_with_desc {
                    run += 1;
                } else if !lines[k].trim().is_empty() && indent_of(lines[k]) <= indent_of(line) {
                    break;
                }
            }
            in_cmds = run >= 3;
        }
        if in_cmds {
            let mut desc = String::new();
            let mut m = find(cmd_entry(), line);
            if let Some(c) = &m {
                desc = named(c, "desc").to_string();
            } else if let Some(c) = find(cmd_alone(), line) {
                let k = i + 1;
                if k < lines.len()
                    && indent_of(lines[k]) > indent_of(line)
                    && !lines[k].trim().is_empty()
                    && !lines[k].trim_start().starts_with('-')
                {
                    desc = lines[k].trim().to_string();
                    i = k;
                    m = Some(c);
                }
            }
            if let Some(c) = m {
                let names: Vec<String> = named(&c, "names").split(',').map(|n| normalise_cmd_name(n.trim(), name)).collect();
                let primary = names[0].clone();
                if !primary.is_empty() && !primary.ends_with(':') && valid_name(&primary) {
                    let args = named(&c, "args").trim().to_string();
                    let aliases: Vec<String> =
                        names[1..].iter().filter(|a| !a.is_empty() && **a != primary).cloned().collect();
                    let sub = upsert_sub(&mut cmd, &primary, source);
                    if sub.desc.is_empty() {
                        sub.desc = first_sentence(desc.trim_start_matches(['-', ' ']), 90);
                    }
                    sub.aliases = aliases;
                    if !args.is_empty() {
                        sub.takes_files = is_match(usage_file(), &args)
                            || is_match(re!(r"\.\w+>"), &args)
                            || is_match(re!(r"\b[A-Z]*(FILE|ARCHIVE|PATH|DIR)[A-Z]*\b"), &args);
                    }
                }
            }
        }
        i += 1;
    }

    let usage = usage_lines.join("\n");
    let harvest = re!(r"(?<![\w<-])(--[A-Za-z][\w-]*|-[A-Za-z0-9?])(?=[\s\]|=,<\[]|$)(\[?=(<[^>]+>|[A-Z][\w-]*)\]?| (<[^>]+>|[A-Z][A-Z_]+))?");
    for m in find_all(harvest, &usage) {
        let flag = group(&m, 1);
        if cmd.option(flag).is_none() {
            let a3 = group(&m, 3);
            let a4 = group(&m, 4);
            let mut o = Opt { arg: if !a3.is_empty() { Some(a3.into()) } else if !a4.is_empty() { Some(a4.into()) } else { None }, ..Default::default() };
            if let Some(l) = flag.strip_prefix("--") {
                o.longs.push(l.into());
            } else {
                o.shorts.push(flag.trim_start_matches('-').into());
            }
            o.kind = infer_kind(&o);
            cmd.options.push(o);
        }
    }
    if !cmd.subcommands.is_empty() {
        cmd.takes_files = is_match(usage_file(), &usage);
    }
    cmd
}
