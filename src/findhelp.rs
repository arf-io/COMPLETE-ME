//! find(1): value suggestions for its tests, and a plain-English reading of a find command.

use crate::re;
use crate::util::*;

fn zero_arg(t: &str) -> Option<&'static str> {
    Some(match t {
        "-empty" => "is empty (a 0-byte file or an empty directory)",
        "-executable" => "is executable by you",
        "-readable" => "is readable by you",
        "-writable" => "is writable by you",
        "-nouser" => "has no known owner",
        "-nogroup" => "has no known group",
        "-true" => "always true",
        "-false" => "never true",
        "-print" => "print its path",
        "-print0" => "print its path, NUL-terminated (pair with xargs -0)",
        "-ls" => "list it like ls -dils",
        "-delete" => "DELETE it",
        "-prune" => "do not descend into it",
        "-quit" => "stop searching right away",
        "-depth" => "(process directory contents before the directory)",
        "-xdev" | "-mount" => "(stay on this filesystem)",
        "-daystart" => "(measure times from the start of today)",
        "-follow" => "(follow symlinks)",
        "-noleaf" => "(do not optimise for Unix directory link counts)",
        _ => return None,
    })
}
const DELETE_TEXT: &str = "DELETE it";

const ONE_ARG: &[&str] = &[
    "-name", "-iname", "-path", "-ipath", "-wholename", "-iwholename", "-regex", "-iregex", "-type", "-xtype", "-size",
    "-mtime", "-atime", "-ctime", "-mmin", "-amin", "-cmin", "-perm", "-user", "-group", "-uid", "-gid", "-newer",
    "-anewer", "-cnewer", "-links", "-inum", "-samefile", "-lname", "-ilname", "-fstype", "-maxdepth", "-mindepth",
    "-regextype", "-used", "-context", "-printf", "-fprint", "-fprint0", "-fls", "-newermt", "-newerct", "-newerat",
    "-newermm", "-files0-from",
];
const ACTIONS: &[&str] = &[
    "-print", "-print0", "-ls", "-delete", "-prune", "-quit", "-exec", "-execdir", "-ok", "-okdir", "-printf", "-fprint",
    "-fprint0", "-fls", "-fprintf",
];
const EXEC: &[&str] = &["-exec", "-execdir", "-ok", "-okdir"];
const TYPES: &[(&str, &str)] = &[
    ("f", "regular file"), ("d", "directory"), ("l", "symbolic link"), ("p", "named pipe (FIFO)"), ("s", "socket"),
    ("b", "block device"), ("c", "character device"),
];

fn unit(u: &str) -> &'static str {
    match u {
        "c" => "bytes",
        "k" => "KiB",
        "M" => "MiB",
        "G" => "GiB",
        "w" => "2-byte words",
        _ => "512-byte blocks",
    }
}

fn pairs(v: &[(&str, &str)]) -> Vec<(String, String)> {
    v.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
}

// ------------------------------------------------------------ value suggestions

pub fn extensions_here(limit: usize) -> Vec<(String, String)> {
    let mut counts: Vec<(String, usize)> = vec![];
    if let Ok(rd) = std::fs::read_dir(".") {
        for (i, e) in rd.flatten().enumerate() {
            if i > 3000 {
                break;
            }
            let name = e.file_name().to_string_lossy().into_owned();
            // os.path.splitext: leading dots don't start an extension
            let trimmed = name.trim_start_matches('.');
            let ext = match trimmed.rfind('.') {
                Some(p) => &trimmed[p..],
                None => "",
            };
            if !ext.is_empty() && char_len(ext) <= 10 && !name.starts_with('.') {
                match counts.iter_mut().find(|(e, _)| e == ext) {
                    Some(c) => c.1 += 1,
                    None => counts.push((ext.to_string(), 1)),
                }
            }
        }
    }
    counts.sort_by_key(|c| std::cmp::Reverse(c.1)); // stable: ties keep first-seen order, like Counter.most_common
    counts
        .into_iter()
        .take(limit)
        .map(|(e, n)| (format!("*{e}"), format!("{n} {} like this here", if n == 1 { "file" } else { "files" })))
        .collect()
}

pub fn values_for(prev: &str) -> Vec<(String, String)> {
    match prev {
        "-type" | "-xtype" => {
            let mut v = pairs(TYPES);
            v.push(("f,l".into(), "regular file or symlink (GNU)".into()));
            v
        }
        "-size" => pairs(&[
            ("+100M", "bigger than 100 MiB"), ("+1G", "bigger than 1 GiB"), ("-1k", "smaller than 1 KiB"),
            ("+10M", "bigger than 10 MiB"), ("0", "exactly zero size"), ("-100c", "under 100 bytes"),
        ]),
        "-mtime" | "-atime" | "-ctime" => {
            let verb = match prev { "-mtime" => "modified", "-atime" => "accessed", _ => "status-changed" };
            vec![
                ("-1".into(), format!("{verb} in the last 24 hours")),
                ("-7".into(), format!("{verb} in the last week")),
                ("+30".into(), format!("{verb} more than 30 days ago")),
                ("+365".into(), format!("{verb} more than a year ago")),
                ("0".into(), format!("{verb} between now and 24 hours ago")),
            ]
        }
        "-mmin" | "-amin" | "-cmin" => pairs(&[("-5", "in the last 5 minutes"), ("-60", "in the last hour"), ("+1440", "more than a day ago")]),
        "-perm" => pairs(&[
            ("-u+x", "executable by its owner"), ("/a+x", "executable by anyone"), ("-o+w", "world-writable ⚠"),
            ("644", "exactly rw-r--r--"), ("755", "exactly rwxr-xr-x"), ("600", "exactly rw-------"),
            ("-4000", "setuid bit set"), ("-2000", "setgid bit set"), ("/6000", "setuid or setgid"),
        ]),
        "-name" | "-iname" | "-lname" | "-ilname" => {
            let mut v = extensions_here(12);
            v.extend(pairs(&[("*.log", "log files"), (".*", "hidden files"), ("*~", "editor backups"), ("*.bak", "backup files"), ("*.tmp", "temporary files")]));
            v
        }
        "-path" | "-ipath" | "-wholename" => pairs(&[
            ("*/node_modules/*", "anything inside node_modules"), ("*/.git/*", "anything inside .git"),
            ("./build/*", "inside ./build"), ("*/target/*", "inside a cargo target dir"),
        ]),
        "-maxdepth" | "-mindepth" => pairs(&[("1", "this directory only"), ("2", "one level of subdirectories"), ("3", "two levels")]),
        "-regextype" => pairs(&[
            ("posix-extended", "like grep -E (recommended)"), ("posix-basic", "like grep"), ("emacs", "the default"),
            ("posix-egrep", "like egrep"),
        ]),
        "-printf" => pairs(&[
            (r"%p\n", "path"), (r"%s\t%p\n", "size in bytes, path"), (r"%TY-%Tm-%Td %TH:%TM %p\n", "modified date, path"),
            (r"%M %u %g %p\n", "permissions, owner, group, path"), (r"%f\n", "file name only"), (r"%h\n", "parent directory"),
        ]),
        "-newermt" | "-newerct" | "-newerat" => pairs(&[
            ("today", "since midnight"), ("yesterday", "since yesterday midnight"), ("1 hour ago", "in the last hour"),
            ("2026-01-01", "since this date"),
        ]),
        _ => vec![],
    }
}

pub fn operators() -> Vec<(String, String)> {
    pairs(&[
        ("(", "start a group: \\( … \\) (needed around -o before an action)"), (")", "end the group"),
        ("-o", "OR: either side"), ("-a", "AND (the default between tests)"), ("!", "NOT: invert the next test"),
        ("-not", "NOT: invert the next test"), ("-prune", "do not descend (e.g. -path ./.git -prune -o …)"),
    ])
}

// ------------------------------------------------------------ explanation

#[derive(Debug)]
enum Node {
    Leaf { text: String, action: bool },
    And(Vec<Node>),
    Or(Vec<Node>),
    Not(Box<Node>),
    Group(Box<Node>),
}

/// Python repr() of a string.
fn py_repr(s: &str) -> String {
    let q = if s.contains('\'') && !s.contains('"') { '"' } else { '\'' };
    let mut out = String::new();
    out.push(q);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c if c == q => {
                out.push('\\');
                out.push(c);
            }
            c => out.push(c),
        }
    }
    out.push(q);
    out
}

fn size_text(arg: &str) -> String {
    match find(re!(r"^([+-]?)(\d+)([ckMGbw]?)$"), arg) {
        None => format!("has size {arg}"),
        Some(m) => {
            let rel = match group(&m, 1) { "+" => "bigger than", "-" => "smaller than", _ => "exactly" };
            format!("is {rel} {} {}", group(&m, 2), unit(group(&m, 3)))
        }
    }
}

fn days_text(arg: &str, verb: &str, u: &str) -> String {
    match find(re!(r"^([+-]?)(\d+)$"), arg) {
        None => format!("was {verb} {arg} {u}"),
        Some(m) => {
            let n = group(&m, 2);
            match group(&m, 1) {
                "-" => format!("was {verb} less than {n} {u} ago"),
                "+" => format!("was {verb} more than {n} {u} ago"),
                _ => format!("was {verb} {n} {u} ago (rounded down)"),
            }
        }
    }
}

fn primary_text(tok: &str, args: &[String]) -> String {
    let a = args.first().map(|s| s.as_str()).unwrap_or("?");
    let r = py_repr(a);
    match tok {
        "-type" | "-xtype" => {
            let kinds: Vec<String> = a
                .split(',')
                .map(|k| TYPES.iter().find(|(c, _)| *c == k).map(|(_, d)| d.to_string()).unwrap_or_else(|| k.to_string()))
                .collect();
            return format!("is a {}", kinds.join(" or "));
        }
        "-size" => return size_text(a),
        "-mtime" | "-atime" | "-ctime" => {
            let v = match tok { "-mtime" => "modified", "-atime" => "accessed", _ => "changed" };
            return days_text(a, v, "days");
        }
        "-mmin" | "-amin" | "-cmin" => {
            let v = match tok { "-mmin" => "modified", "-amin" => "accessed", _ => "changed" };
            return days_text(a, v, "minutes");
        }
        t if EXEC.contains(&t) => {
            let has_end = matches!(args.last().map(|s| s.as_str()), Some(";") | Some("+"));
            let end = if has_end { args.last().unwrap().as_str() } else { ";" };
            let body = if has_end { &args[..args.len() - 1] } else { args };
            let cmdline: Vec<&str> = body.iter().map(|x| if x == "{}" { "FILE" } else { x.as_str() }).collect();
            let how = if end == "+" { "once with MANY files at a time (fast)" } else { "once per file" };
            let wh = if t == "-execdir" || t == "-okdir" { " from inside the file's directory" } else { "" };
            let ask = if t == "-ok" || t == "-okdir" { ", asking you first" } else { "" };
            return format!("run `{}` {how}{wh}{ask}", cmdline.join(" "));
        }
        _ => {}
    }
    if let Some(z) = zero_arg(tok) {
        return z.into();
    }
    match tok {
        "-name" => format!("its name matches {r}"),
        "-iname" => format!("its name matches {r} (any case)"),
        "-path" => format!("its path matches {r}"),
        "-ipath" => format!("its path matches {r} (any case)"),
        "-wholename" => format!("its path matches {r}"),
        "-regex" => format!("its WHOLE path matches the regex {r}"),
        "-iregex" => format!("its WHOLE path matches the regex {r} (any case)"),
        "-user" => format!("is owned by user {a}"),
        "-group" => format!("belongs to group {a}"),
        "-newer" => format!("is newer than {a}"),
        "-maxdepth" => format!("(look at most {a} level(s) deep)"),
        "-mindepth" => format!("(skip the first {a} level(s))"),
        "-regextype" => format!("(regexes use {a} syntax)"),
        "-perm" => format!("has permissions {a}"),
        "-links" => format!("has {a} hard links"),
        "-samefile" => format!("is the same file as {a}"),
        "-fstype" => format!("is on a {a} filesystem"),
        "-printf" => format!("print using the format {r}"),
        "-fprint" => format!("write its path to the file {a}"),
        "-newermt" => format!("was modified after {r}"),
        _ => format!("{tok} {}", args.join(" ")).trim().to_string(),
    }
}

struct Parser<'a> {
    toks: &'a [String],
    pos: usize,
    warnings: Vec<String>,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<&str> {
        self.toks.get(self.pos).map(|s| s.as_str())
    }
    fn take(&mut self) -> String {
        self.pos += 1;
        self.toks[self.pos - 1].clone()
    }
    fn or(&mut self) -> Node {
        let mut items = vec![self.and()];
        while matches!(self.peek(), Some("-o") | Some("-or")) {
            self.take();
            items.push(self.and());
        }
        if items.len() == 1 { items.pop().unwrap() } else { Node::Or(items) }
    }
    fn and(&mut self) -> Node {
        let mut items = vec![self.not()];
        while let Some(p) = self.peek() {
            if p == "-o" || p == "-or" || p == ")" {
                break;
            }
            if p == "-a" || p == "-and" {
                self.take();
            }
            items.push(self.not());
        }
        if items.len() == 1 { items.pop().unwrap() } else { Node::And(items) }
    }
    fn not(&mut self) -> Node {
        if matches!(self.peek(), Some("!") | Some("-not")) {
            self.take();
            return Node::Not(Box::new(self.not()));
        }
        self.primary()
    }
    fn primary(&mut self) -> Node {
        if self.peek().is_none() {
            // Python would raise IndexError here; treat a dangling operator as an empty test.
            self.warnings.push("expression ends with an operator".into());
            return Node::Leaf { text: "?".into(), action: false };
        }
        let tok = self.take();
        if tok == "(" {
            let inner = self.or();
            if self.peek() == Some(")") {
                self.take();
            } else {
                self.warnings.push("a \\( is never closed with \\)".into());
            }
            return Node::Group(Box::new(inner));
        }
        if EXEC.contains(&tok.as_str()) {
            let mut args: Vec<String> = vec![];
            let mut terminated = false;
            while self.peek().is_some() {
                let t = self.take();
                args.push(t.clone());
                if t == ";" || (t == "+" && args.len() > 1 && args[args.len() - 2] == "{}") {
                    terminated = true;
                    break;
                }
            }
            if !terminated {
                self.warnings.push(format!("{tok} is missing its terminator: end it with \\; (or {{}} +)"));
            }
            if !args.iter().any(|a| a == "{}") {
                self.warnings.push(format!("{tok} never mentions {{}}, so the command won't see the file names"));
            }
            return Node::Leaf { text: primary_text(&tok, &args), action: true };
        }
        let mut args = vec![];
        if ONE_ARG.contains(&tok.as_str()) && self.peek().is_some() {
            args = vec![self.take()];
        } else if tok == "-fprintf" {
            if self.pos + 1 < self.toks.len() {
                args = vec![self.take(), self.take()];
            }
        } else if zero_arg(&tok).is_none() {
            self.warnings.push(format!("unknown find primary {}", py_repr(&tok)));
        }
        let action = ACTIONS.contains(&tok.as_str());
        Node::Leaf { text: primary_text(&tok, &args), action }
    }
}

fn has_action(n: &Node) -> bool {
    match n {
        Node::Leaf { action, .. } => *action,
        Node::And(v) | Node::Or(v) => v.iter().any(has_action),
        Node::Not(b) | Node::Group(b) => has_action(b),
    }
}

fn lint(n: &Node, w: &mut Vec<String>) {
    match n {
        Node::Or(items) => {
            if items.iter().any(has_action) && !items.iter().all(has_action) {
                w.push("an action sits inside only one side of -o, so it only runs for that side. Wrap the tests: \\( -name a -o -name b \\) -exec …".into());
            }
            for c in items {
                lint(c, w);
            }
        }
        Node::And(items) => {
            let mut seen_delete = false;
            for c in items {
                match c {
                    Node::Leaf { text, .. } if text == DELETE_TEXT => seen_delete = true,
                    c if seen_delete && !matches!(c, Node::Leaf { action: true, .. }) => {
                        w.push("-delete comes BEFORE a test: it deletes everything it reaches. Put -delete last.".into());
                        break;
                    }
                    _ => {}
                }
            }
            for c in items {
                lint(c, w);
            }
        }
        Node::Not(b) | Node::Group(b) => lint(b, w),
        Node::Leaf { .. } => {}
    }
}

fn render(n: &Node, depth: usize) -> Vec<String> {
    let pad = "  ".repeat(depth);
    match n {
        Node::Leaf { text, .. } => vec![format!("{pad}{}{text}", if text == DELETE_TEXT { "⚠ " } else { "" })],
        Node::Group(b) => render(b, depth),
        Node::Not(b) => {
            let mut v = vec![format!("{pad}NOT:")];
            v.extend(render(b, depth + 1));
            v
        }
        Node::And(items) | Node::Or(items) => {
            let is_and = matches!(n, Node::And(_));
            let mut v = vec![format!("{pad}{}", if is_and { "ALL of:" } else { "ANY of:" })];
            for c in items {
                match c {
                    Node::Group(inner) if matches!((&**inner, is_and), (Node::And(_), true) | (Node::Or(_), false)) => {
                        if let Node::And(gi) | Node::Or(gi) = &**inner {
                            for g in gi {
                                v.extend(render(g, depth + 1));
                            }
                        }
                    }
                    c => v.extend(render(c, depth + 1)),
                }
            }
            v
        }
    }
}

pub fn explain(tokens: &[String]) -> String {
    let mut toks: Vec<String> = tokens.to_vec();
    if toks.first().map(|t| t.rsplit('/').next().unwrap_or(t) == "find").unwrap_or(false) {
        toks.remove(0);
    }
    while toks.first().map(|t| matches!(t.as_str(), "-H" | "-L" | "-P")).unwrap_or(false) {
        toks.remove(0);
    }
    let mut starts = vec![];
    while let Some(t) = toks.first() {
        if t.starts_with('-') || t == "(" || t == "!" || t == ")" {
            break;
        }
        starts.push(toks.remove(0));
    }
    let mut p = Parser { toks: &toks, pos: 0, warnings: vec![] };
    let tree = if toks.is_empty() { None } else { Some(p.or()) };
    if p.pos < toks.len() {
        let t = py_repr(&toks[p.pos]);
        p.warnings.push(format!("unexpected {t} (unbalanced parentheses?)"));
    }
    let mut warnings = std::mem::take(&mut p.warnings);
    let mut lines = vec![format!(
        "Search in: {}",
        if starts.is_empty() { ". (current directory)".to_string() } else { starts.join(", ") }
    )];
    match &tree {
        None => lines.push("Match: everything, and print each path".into()),
        Some(t) => {
            lint(t, &mut warnings);
            lines.push("For each file/directory found, keep going while:".into());
            lines.extend(render(t, 1));
            if !has_action(t) {
                lines.push("…and print the path of every match (the default action)".into());
            }
        }
    }
    let mut seen: Vec<String> = vec![];
    for w in warnings {
        if !seen.contains(&w) {
            lines.push(format!("⚠ {w}"));
            seen.push(w);
        }
    }
    lines.join("\n")
}

pub fn run(args: &[String]) -> i32 {
    let Some(sub) = args.first() else {
        eprintln!("usage: complete-me find <explain|values|operators> …");
        return 2;
    };
    let rest: Vec<String> = match args[1..].iter().position(|a| a == "--") {
        Some(i) => {
            let mut v = args[1..1 + i].to_vec();
            v.extend(args[2 + i..].iter().cloned());
            v
        }
        None => args[1..].to_vec(),
    };
    match sub.as_str() {
        "explain" => println!("{}", explain(&rest)),
        "values" => {
            let prev = rest.first().cloned().unwrap_or_default();
            let v: Vec<String> = values_for(&prev).into_iter().map(|(x, d)| format!("{x}\t{d}")).collect();
            println!("{}", v.join("\n"));
        }
        "operators" => {
            let v: Vec<String> = operators().into_iter().map(|(x, d)| format!("{x}\t{d}")).collect();
            println!("{}", v.join("\n"));
        }
        other => {
            eprintln!("complete-me find: unknown subcommand {other:?}");
            return 2;
        }
    }
    0
}
