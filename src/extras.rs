//! The fish-only extras, for bash and zsh: one engine both shells ask on TAB.
//!
//!   complete-me extras --shell bash|zsh [--history] -- LINE-LEFT-OF-CURSOR
//!
//! With --history, recent history lines (most recent first) arrive on stdin. Output: a first line
//! `replace` (the shell should offer only these) or `add` (offer these alongside its own) plus a TAB
//! and the word under the cursor as typed, then
//! `group<TAB>word<TAB>description` lines. Words are complete, already quoted for the shell where
//! quoting matters (regex patterns), and filtered by what has been typed so far.
use crate::util::{sq, Args};
use crate::{findhelp, re, rx, sshopts};
use std::time::Duration;

#[derive(Debug, Default, PartialEq)]
pub struct Line {
    /// Words of the current simple command before the cursor word (unquoted), wrappers removed.
    pub words: Vec<String>,
    /// The word under the cursor exactly as typed (quotes included).
    pub raw: String,
    /// The same word unquoted.
    pub cur: String,
}

const WRAPPERS: &[&str] = &["sudo", "doas", "command", "exec", "time", "nice", "nohup", "builtin", "noglob"];

/// Split the text left of the cursor into the current command's words. Lenient: an unterminated
/// quote is the word being typed; `|`, `;`, `&&`, `||`, `&`, `(` start a new command.
pub fn split_line(s: &str) -> Line {
    let mut words: Vec<String> = vec![];
    let (mut cur, mut raw) = (String::new(), String::new());
    let mut have = false;
    let mut chars = s.chars().peekable();
    let flush = |words: &mut Vec<String>, cur: &mut String, raw: &mut String, have: &mut bool| {
        if *have {
            words.push(std::mem::take(cur));
        }
        raw.clear();
        *have = false;
    };
    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                have = true;
                raw.push(c);
                for n in chars.by_ref() {
                    raw.push(n);
                    if n == '\'' {
                        break;
                    }
                    cur.push(n);
                }
            }
            '"' => {
                have = true;
                raw.push(c);
                while let Some(n) = chars.next() {
                    raw.push(n);
                    match n {
                        '"' => break,
                        '\\' => match chars.peek() {
                            Some(&q) if "\\\"$`".contains(q) => {
                                raw.push(q);
                                cur.push(q);
                                chars.next();
                            }
                            _ => cur.push('\\'),
                        },
                        n => cur.push(n),
                    }
                }
            }
            '\\' => {
                have = true;
                raw.push(c);
                if let Some(n) = chars.next() {
                    raw.push(n);
                    cur.push(n);
                }
            }
            '|' | ';' | '&' | '(' | '\n' => {
                flush(&mut words, &mut cur, &mut raw, &mut have);
                words.clear();
            }
            c if c.is_whitespace() => flush(&mut words, &mut cur, &mut raw, &mut have),
            c => {
                have = true;
                raw.push(c);
                cur.push(c);
            }
        }
    }
    // Leading VAR=value assignments and wrappers like sudo are not the command.
    let mut start = 0;
    while start < words.len() {
        let w = &words[start];
        let assign = w.find('=').is_some_and(|i| i > 0 && w[..i].chars().all(|c| c.is_alphanumeric() || c == '_'));
        if assign || WRAPPERS.contains(&w.as_str()) {
            start += 1;
        } else {
            break;
        }
    }
    Line { words: words[start..].to_vec(), raw, cur }
}

fn basename(s: &str) -> &str {
    s.rsplit('/').next().unwrap_or(s)
}

// ------------------------------------------------------------ slots

const REGEX_TOOLS: &[&str] = &["grep", "egrep", "rgrep", "zgrep", "ugrep", "rg", "sed", "gsed"];

/// True when the cursor is where the pattern goes (mirrors the fish runtime's rule).
pub fn regex_slot(words: &[String]) -> bool {
    let Some(tool) = words.first().map(|w| basename(w)) else { return false };
    let prev = words.last().map(|s| s.as_str()).unwrap_or("");
    if tool == "find" {
        return prev == "-regex" || prev == "-iregex";
    }
    if !REGEX_TOOLS.contains(&tool) {
        return false;
    }
    let argopts: &[&str] = match tool {
        "rg" => &["-A", "-B", "-C", "-m", "-d", "-g", "-t", "-T", "-j", "-M", "-r", "-E", "--max-count", "--glob", "--iglob",
            "--type", "--type-not", "--threads", "--max-columns", "--replace", "--max-depth", "--encoding", "--sort", "--sortr",
            "--pre", "--pre-glob", "--colors"],
        "sed" | "gsed" => &["-l"],
        _ => &["-A", "-B", "-C", "-m", "-d", "-D", "--max-count", "--include", "--exclude", "--exclude-dir", "--label",
            "--group-separator"],
    };
    if ["-e", "--regexp", "--expression"].contains(&prev) {
        return true;
    }
    let mut skip = false;
    for t in &words[1..] {
        if skip {
            skip = false;
            continue;
        }
        match t.as_str() {
            "-e" | "--regexp" | "-f" | "--file" | "--expression" => return false,
            t if t.starts_with('-') => skip = argopts.contains(&t),
            _ => return false,
        }
    }
    true
}

/// `Some(block)` when the cursor is inside an open `find … -exec CMD …` (words after -exec).
pub fn find_exec_block(words: &[String]) -> Option<Vec<String>> {
    let mut idx = None;
    for (i, t) in words.iter().enumerate() {
        match t.as_str() {
            "-exec" | "-execdir" | "-ok" | "-okdir" => idx = Some(i),
            ";" => idx = None,
            "+" if i > 0 && words[i - 1] == "{}" => idx = None,
            _ => {}
        }
    }
    idx.map(|i| words[i + 1..].to_vec())
}

const FIND_VALUED: &[&str] = &["-type", "-xtype", "-size", "-mtime", "-atime", "-ctime", "-mmin", "-amin", "-cmin", "-perm",
    "-name", "-iname", "-lname", "-ilname", "-path", "-ipath", "-wholename", "-maxdepth", "-mindepth", "-regextype", "-printf",
    "-newermt", "-newerct", "-newerat"];

// ------------------------------------------------------------ clipboard

fn read_stdout(argv: &[&str], timeout: Duration) -> String {
    use std::io::Read;
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};
    let Ok(mut child) = Command::new(argv[0]).args(&argv[1..]).stdin(Stdio::null()).stdout(Stdio::piped())
        .stderr(Stdio::null()).process_group(0).spawn() else { return String::new() };
    let pid = child.id() as i32;
    let mut so = child.stdout.take().unwrap();
    let t = std::thread::spawn(move || {
        let mut b = Vec::new();
        let _ = so.by_ref().take(8192).read_to_end(&mut b);
        b
    });
    let start = std::time::Instant::now();
    while let Ok(None) = child.try_wait() {
        if start.elapsed() > timeout {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    unsafe { libc::kill(-pid, libc::SIGKILL) };
    let _ = child.wait();
    String::from_utf8_lossy(&t.join().unwrap_or_default()).into_owned()
}

pub fn clipboard_text() -> String {
    if let Ok(t) = std::env::var("COMPLETE_ME_CLIPBOARD") {
        return t;
    }
    let set = |v: &str| std::env::var_os(v).is_some_and(|x| !x.is_empty());
    let have = |c: &str| crate::util::which(c).is_some();
    let t = Duration::from_millis(400);
    if set("WAYLAND_DISPLAY") && have("wl-paste") {
        read_stdout(&["wl-paste", "-n", "-t", "text"], t)
    } else if set("DISPLAY") && have("xclip") {
        read_stdout(&["xclip", "-o", "-selection", "clipboard"], t)
    } else if set("DISPLAY") && have("xsel") {
        read_stdout(&["xsel", "-ob"], t)
    } else if have("pbpaste") {
        read_stdout(&["pbpaste"], t)
    } else {
        String::new()
    }
}

fn url_unescape(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The clipboard as a candidate, only when it is ONE line that fits the slot (want: git|url|path|dir).
/// Passwords, prose and code never qualify.
pub fn clip(text: &str, want: &str) -> Option<(String, String)> {
    let text: String = text.chars().take(4096).collect();
    let lines: Vec<&str> = text.trim().lines().collect();
    if lines.len() != 1 {
        return None;
    }
    let mut c = lines[0].trim().to_string();
    if c.is_empty() {
        return None;
    }
    let exists = |p: &str| std::path::Path::new(p).exists();
    if c.contains(' ') && !c.starts_with("file://") && !exists(&c) {
        return None;
    }
    if let Some(r) = c.strip_prefix("file://") {
        c = url_unescape(r.strip_prefix("localhost").unwrap_or(r));
    }
    let home = crate::util::home().to_string_lossy().into_owned();
    let expand = |p: &str| if let Some(r) = p.strip_prefix('~') { format!("{home}{r}") } else { p.to_string() };
    match want {
        "git" => {
            let shape = re!(r"^(git@[\w.-]+:[\w./~-]+|(ssh|git|https?)://[\w.@:-]+/[\w./~-]+)$");
            let gitty = re!(r"\.git/?$|^git@|^ssh://|^git://|github\.com|gitlab|bitbucket|codeberg|sr\.ht|gitea|forgejo|//git\.");
            if crate::util::is_match(shape, &c) && crate::util::is_match(gitty, &c) {
                return Some((c.trim_end_matches('/').to_string(), "git URL from clipboard".into()));
            }
            None
        }
        "url" => crate::util::is_match(re!(r"^(https?|ftp)://\S+$"), &c).then(|| (c.clone(), "URL from clipboard".into())),
        _ => {
            let p = expand(&c);
            let md = std::fs::metadata(&p).ok()?;
            if md.is_dir() {
                Some((format!("{}/", c.trim_end_matches('/')), "directory from clipboard".into()))
            } else if want == "path" {
                Some((c, "file from clipboard".into()))
            } else {
                None
            }
        }
    }
}

const URL_CMDS: &[&str] = &["curl", "wget", "xh", "http", "https", "aria2c", "yt-dlp", "mpv", "xdg-open", "open", "firefox",
    "chromium", "links", "lynx", "w3m"];
const PATH_CMDS: &[&str] = &["cat", "less", "more", "bat", "vim", "nvim", "vi", "nano", "hx", "code", "subl", "cp", "mv", "ln",
    "tar", "unzip", "file", "stat", "du", "ls", "head", "tail", "diff", "source", ".", "rsync", "scp", "chmod", "chown", "wc",
    "sha256sum", "md5sum"];

/// Which clipboard shape fits here, if any.
pub fn clip_want(words: &[String]) -> Option<&'static str> {
    let cmd = basename(words.first()?);
    let has = |w: &str| words.iter().skip(1).any(|x| x == w);
    match cmd {
        "git" if has("clone") || has("remote") && (has("add") || has("set-url")) || has("submodule") && has("add") => Some("git"),
        "cd" | "pushd" => Some("dir"),
        c if URL_CMDS.contains(&c) => Some("url"),
        c if PATH_CMDS.contains(&c) => Some("path"),
        _ => None,
    }
}

// ------------------------------------------------------------ history

/// Options of `cmd` that take a value, from its generated model (`-p`, `--port`, `-name`).
pub fn valued_opts(cmd: &str) -> Vec<String> {
    let Ok(json) = std::fs::read_to_string(crate::util::cache_dir("json").join(format!("{cmd}.json"))) else { return vec![] };
    let mut out = vec![];
    let r = re!(r#"\{"shorts":\[([^\]]*)\],"longs":\[([^\]]*)\],"olds":\[([^\]]*)\],"arg":""#);
    for c in crate::util::find_all(r, &json) {
        for (i, dash) in [(1, "-"), (2, "--"), (3, "-")] {
            for n in crate::util::group(&c, i).split(',').map(|s| s.trim_matches('"')).filter(|s| !s.is_empty()) {
                out.push(format!("{dash}{n}"));
            }
        }
    }
    out
}

/// The positional words: options dropped, and so are their values (known from the model, or a
/// bare number after a short option, as in `-p 2222`).
fn positional(ws: &[String], valued: &[String]) -> Vec<String> {
    let mut out = vec![];
    let mut i = 0;
    while i < ws.len() {
        let w = &ws[i];
        if w.starts_with('-') && w.len() > 1 {
            let next_is_value = ws.get(i + 1).is_some_and(|n| {
                !n.starts_with('-') && (valued.contains(w) || w.len() == 2 && !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
            });
            i += if next_is_value && !w.contains('=') { 2 } else { 1 };
            continue;
        }
        out.push(w.clone());
        i += 1;
    }
    out
}

/// Arguments used before at this position: history lines whose positional words start with the
/// ones typed so far contribute the positional words that followed. Most recent first, max 25.
pub fn history_args(history: &[String], words: &[String], cur: &str) -> Vec<(String, String)> {
    let valued = valued_opts(basename(&words[0]));
    let want = positional(words, &valued);
    let mut seen: Vec<String> = vec![];
    for line in history {
        let l = split_line(line);
        if l.words.is_empty() && l.cur.is_empty() {
            continue;
        }
        let mut ws = l.words;
        if !l.cur.is_empty() {
            ws.push(l.cur);
        }
        let pos = positional(&ws, &valued);
        if pos.len() <= want.len() || pos[1..want.len()] != want[1..] || basename(&pos[0]) != basename(&want[0]) {
            continue;
        }
        for t in &pos[want.len()..] {
            if t.starts_with(cur) && !t.is_empty() && !seen.contains(t) {
                seen.push(t.clone());
            }
        }
        if seen.len() >= 25 {
            break;
        }
    }
    seen.into_iter()
        .map(|t| {
            let md = std::fs::metadata(&t);
            match md {
                Ok(m) if m.is_dir() => (format!("{}/", t.trim_end_matches('/')), "directory you used here before".into()),
                Ok(_) => (t, "file you used here before".into()),
                Err(_) => (t, "used here before".into()),
            }
        })
        .collect()
}

/// Values given to option `flag` of `cmd` before (`--flag v` and `--flag=v`), most recent first, max 20.
pub fn history_optvals(history: &[String], cmd: &str, flag: &str, cur: &str) -> Vec<(String, String)> {
    let mut seen: Vec<String> = vec![];
    let eq = format!("{flag}=");
    for line in history {
        let l = split_line(line);
        let mut ws = l.words;
        if !l.cur.is_empty() {
            ws.push(l.cur);
        }
        if ws.first().map(|w| basename(w)) != Some(basename(cmd)) {
            continue;
        }
        for i in 1..ws.len() {
            let v = if ws[i] == flag && i + 1 < ws.len() && !ws[i + 1].starts_with('-') {
                ws[i + 1].clone()
            } else if let Some(v) = ws[i].strip_prefix(&eq) {
                v.to_string()
            } else {
                continue;
            };
            if v.starts_with(cur) && !v.is_empty() && !seen.contains(&v) {
                seen.push(v);
            }
        }
        if seen.len() >= 20 {
            break;
        }
    }
    seen.into_iter().map(|v| (v, format!("given to {flag} before"))).collect()
}

// ------------------------------------------------------------ the engine

pub struct Out {
    pub replace: bool,
    pub rows: Vec<(&'static str, String, String)>,
}

fn starts(rows: Vec<(String, String)>, cur: &str) -> Vec<(String, String)> {
    rows.into_iter().filter(|(v, _)| v.starts_with(cur)).collect()
}

pub fn compute(line: &Line, history: &[String], want_clip: bool) -> Out {
    let mut out = Out { replace: false, rows: vec![] };
    let words = &line.words;
    let cur = line.cur.as_str();
    let Some(first) = words.first() else { return out };
    let cmd = basename(first);
    let prev = words.last().map(|s| s.as_str()).unwrap_or("");
    let push = |out: &mut Out, g: &'static str, rows: Vec<(String, String)>| {
        out.rows.extend(rows.into_iter().map(|(v, d)| (g, v, d)));
    };

    // Regex patterns: only these make sense in the pattern slot. Quoted, so they paste safely.
    if regex_slot(words) {
        let rows = rx::candidates(words, &line.raw).into_iter().map(|(v, d)| (sq(&v), d)).collect();
        push(&mut out, "regex", rows);
        out.replace = true;
        return out;
    }
    if cmd == "find" {
        if let Some(block) = find_exec_block(words) {
            if block.is_empty() {
                return out; // the command after -exec: the shell completes commands
            }
            let mut rows = vec![];
            if !block.iter().any(|w| w == "{}") {
                rows.push(("{}".to_string(), "the path of each file find found".to_string()));
            } else {
                rows.push(("\\;".to_string(), "end -exec: run the command once per file".to_string()));
                if block.last().map(|s| s.as_str()) == Some("{}") {
                    rows.push(("+".to_string(), "end -exec: one run with many files (faster)".to_string()));
                }
            }
            push(&mut out, "find", starts(rows, cur));
            return out;
        }
        if FIND_VALUED.contains(&prev) {
            push(&mut out, "find", starts(findhelp::values_for(prev), cur));
            out.replace = !out.rows.is_empty();
            if out.replace {
                return out;
            }
        } else if words.iter().skip(1).any(|w| w.starts_with('-')) && (cur.is_empty() || cur.starts_with('-') || "()!".contains(cur)) {
            let ops = findhelp::operators().into_iter().map(|(v, d)| (if v == "(" || v == ")" { format!("\\{v}") } else { v }, d));
            push(&mut out, "find", starts(ops.collect(), cur));
        }
    }
    if ["ssh", "scp", "sftp"].contains(&cmd) {
        if prev == "-o" {
            push(&mut out, "ssh", starts(sshopts::candidates(cur, false), cur));
            out.replace = true;
            return out;
        }
        if let Some(rest) = cur.strip_prefix("-o") {
            let rows = sshopts::candidates(rest, false).into_iter().map(|(v, d)| (format!("-o{v}"), d)).collect();
            let rows = starts(rows, cur);
            push(&mut out, "ssh", rows);
            out.replace = true;
            return out;
        }
    }
    if cur.starts_with('-') {
        return out;
    }
    if want_clip {
        if let Some(w) = clip_want(words) {
            if let Some((v, d)) = clip(&clipboard_text(), w) {
                if v.starts_with(cur) {
                    push(&mut out, "clip", vec![(v, d)]);
                }
            }
        }
    }
    if !history.is_empty() {
        if prev.starts_with('-') && !prev.contains('=') && prev.len() > 1 {
            push(&mut out, "histopt", history_optvals(history, first, prev, cur));
        }
        push(&mut out, "hist", history_args(history, words, cur));
    }
    out
}

// ------------------------------------------------------------ bash: what readline actually replaces

/// The unquoted value of a row (regex rows arrive single-quoted, find's `\;` and `\(` escaped).
fn bare(g: &str, v: &str) -> String {
    match g {
        "regex" if v.len() >= 2 && v.starts_with('\'') && v.ends_with('\'') => v[1..v.len() - 1].replace("'\\''", "'"),
        "find" if v.starts_with('\\') => v[1..].to_string(),
        _ => v.to_string(),
    }
}

/// Inside double quotes, `\`, `$`, `` ` `` and `"` need a backslash.
fn dq_escape(s: &str) -> String {
    let mut o = String::new();
    for c in s.chars() {
        if "\\$`\"".contains(c) {
            o.push('\\');
        }
        o.push(c);
    }
    o
}

/// What bash should put in COMPREPLY for a row. Readline replaces only part of the word: after an
/// opening quote (and then word breaks inside the quote don't count), or else after the last
/// COMP_WORDBREAKS character (`=`, `:` …). So the word is rewritten to continue from there.
pub fn bash_word(raw: &str, g: &str, v: &str, breaks: &str) -> String {
    let b = bare(g, v);
    let open = matches!(g, "regex" | "find" | "hist" | "histopt" | "clip" | "ssh")
        .then(|| raw.chars().next().filter(|q| (*q == '\'' || *q == '"') && !raw[1..].contains(*q)))
        .flatten();
    let more_to_type = b.ends_with('/') || b.ends_with('=');
    match open {
        Some('\'') => format!("{}{}", b.replace('\'', "'\\''"), if more_to_type { "" } else { "'" }),
        Some(_) => format!("{}{}", dq_escape(&b), if more_to_type { "" } else { "\"" }),
        None => {
            let full = if matches!(g, "regex" | "find") { v.to_string() } else { crate::util::shquote(&b) };
            let full = if full == "''" { b } else { full };
            let cut = raw.char_indices()
                .rfind(|(_, c)| breaks.contains(*c) && !c.is_whitespace() && *c != '\'' && *c != '"')
                .map(|(i, c)| i + c.len_utf8())
                .unwrap_or(0);
            match full.get(..cut) {
                Some(p) if cut > 0 && p == &raw[..cut] => full[cut..].to_string(),
                _ => full,
            }
        }
    }
}

/// `complete-me clip git|url|path|dir`: the clipboard as one `value<TAB>description` line, if it fits.
pub fn run_clip(args: &[String]) -> i32 {
    let Some(want) = args.first() else {
        eprintln!("usage: complete-me clip git|url|path|dir");
        return 2;
    };
    match clip(&clipboard_text(), want) {
        Some((v, d)) => {
            println!("{v}\t{d}");
            0
        }
        None => 1,
    }
}

pub fn run(args: &[String]) -> i32 {
    let a = Args::parse(args, &["--shell", "--breaks"]);
    let text = a.rest.join(" ");
    let history: Vec<String> = if a.has("--history") {
        use std::io::Read;
        let mut s = String::new();
        let _ = std::io::stdin().take(4_000_000).read_to_string(&mut s);
        // bash/zsh `fc -ln` indents entries; keep the 500 most recent non-empty lines.
        s.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).take(500).collect()
    } else {
        vec![]
    };
    let line = split_line(&text);
    let out = compute(&line, &history, !a.has("--no-clipboard"));
    // The header carries the word under the cursor as typed. For bash, rows are already rewritten
    // to replace exactly what readline replaces (see bash_word).
    let mut s = format!("{}\t{}\n", if out.replace { "replace" } else { "add" }, line.raw);
    let bash = a.value("--shell").as_deref() == Some("bash");
    let breaks = a.value("--breaks").unwrap_or_else(|| "\"'><=;|&(:".to_string());
    for (g, v, d) in out.rows {
        let v = if bash { bash_word(&line.raw, g, &v, &breaks) } else { v };
        s.push_str(&format!("{g}\t{v}\t{}\n", d.replace(['\t', '\n'], " ")));
    }
    print!("{s}");
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    fn w(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn splits_the_current_command() {
        let l = split_line("ls | sudo FOO=1 grep -i 'ab\\d");
        assert_eq!(l.words, w(&["grep", "-i"]));
        assert_eq!(l.raw, "'ab\\d");
        assert_eq!(l.cur, "ab\\d");
        let l = split_line("git clone ");
        assert_eq!(l.words, w(&["git", "clone"]));
        assert_eq!(l.cur, "");
    }

    #[test]
    fn slots() {
        assert!(regex_slot(&w(&["grep", "-i"])));
        assert!(regex_slot(&w(&["rg", "-g", "*.rs"])));
        assert!(!regex_slot(&w(&["grep", "foo"])));
        assert!(regex_slot(&w(&["find", ".", "-regex"])));
        assert_eq!(find_exec_block(&w(&["find", ".", "-exec", "rm"])), Some(w(&["rm"])));
        assert_eq!(find_exec_block(&w(&["find", ".", "-exec", "rm", "{}", ";"])), None);
        assert_eq!(clip_want(&w(&["git", "clone"])), Some("git"));
        assert_eq!(clip_want(&w(&["git", "log"])), None);
    }

    #[test]
    fn clipboard_only_fits_the_slot() {
        assert_eq!(clip("git@github.com:arf-io/COMPLETE-ME.git\n", "git").unwrap().0, "git@github.com:arf-io/COMPLETE-ME.git");
        assert!(clip("hunter2", "git").is_none());
        assert!(clip("two\nlines", "url").is_none());
        assert!(clip("https://example.com/x", "git").is_none());
        assert_eq!(clip("https://example.com/x", "url").unwrap().0, "https://example.com/x");
        assert_eq!(clip("/", "dir").unwrap().0, "/");
        assert!(clip("/definitely/not/here", "path").is_none());
    }

    #[test]
    fn history_ranks_most_recent_first() {
        let h = w(&["ssh -p 2222 beta", "ssh alpha", "ssh -p 22 alpha", "scp x y"]);
        let got: Vec<String> = history_args(&h, &w(&["ssh"]), "").into_iter().map(|r| r.0).collect();
        assert_eq!(got, w(&["beta", "alpha"]));
        let got: Vec<String> = history_optvals(&h, "ssh", "-p", "").into_iter().map(|r| r.0).collect();
        assert_eq!(got, w(&["2222", "22"]));
        let got: Vec<String> = history_args(&h, &w(&["ssh"]), "a").into_iter().map(|r| r.0).collect();
        assert_eq!(got, w(&["alpha"]));
    }

    #[test]
    fn bash_words_continue_what_readline_replaces() {
        let br = "\"'><=;|&(:";
        // inside an open quote: bare, then close the quote; ':' inside the quote is not a break
        assert_eq!(bash_word("'\\d[[:sp", "regex", "'\\d[[:space:]]'", br), "\\d[[:space:]]'");
        assert_eq!(bash_word("\"\\d", "regex", "'\\d$'", br), "\\\\d\\$\"");
        // unquoted: the whole quoted word, unless readline only replaces after a break
        assert_eq!(bash_word("", "regex", "'\\d+'", br), "'\\d+'");
        assert_eq!(bash_word("-oPasswordAuthentication=", "ssh", "-oPasswordAuthentication=yes", br), "yes");
        assert_eq!(bash_word("git@github.com:a", "clip", "git@github.com:arf-io/x.git", br), "arf-io/x.git");
        assert_eq!(bash_word("", "hist", "my file", br), "'my file'");
        assert_eq!(bash_word("'my", "hist", "my file", br), "my file'");
        assert_eq!(bash_word("", "hist", "/tmp/", br), "/tmp/");
        assert_eq!(bash_word("", "find", "\\;", br), "\\;");
    }

    #[test]
    fn engine_modes() {
        let o = compute(&split_line("grep -E "), &[], false);
        assert!(o.replace && o.rows.iter().all(|r| r.0 == "regex" && r.1.starts_with('\'')));
        let o = compute(&split_line("find . -type "), &[], false);
        assert!(o.replace && o.rows.iter().any(|r| r.1 == "f"));
        let o = compute(&split_line("find . -name x -exec rm {} "), &[], false);
        assert_eq!(o.rows.iter().map(|r| r.1.as_str()).collect::<Vec<_>>(), vec!["\\;", "+"]);
        let o = compute(&split_line("ls -"), &w(&["ls /tmp"]), false);
        assert!(o.rows.is_empty());
    }
}
