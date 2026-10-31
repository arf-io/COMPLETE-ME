//! Shared helpers: cached regexes, quoting, sandbox-free subprocess capture, paths, text utils.

use std::io::Read;
use std::path::PathBuf;
use std::process::{Command as Proc, Stdio};
use std::time::{Duration, Instant};

pub use fancy_regex::Regex;

/// A lazily compiled, cached fancy_regex::Regex.  `re!(r"...")`
#[macro_export]
macro_rules! re {
    ($p:expr) => {{
        static R: std::sync::OnceLock<fancy_regex::Regex> = std::sync::OnceLock::new();
        R.get_or_init(|| fancy_regex::Regex::new($p).expect($p))
    }};
}

/// Does `r` match at the very start of `s` (Python's re.match)?  Patterns should start with `^`
/// anyway; this is for patterns used both ways.
pub fn is_match(r: &Regex, s: &str) -> bool {
    r.is_match(s).unwrap_or(false)
}

pub fn find<'a>(r: &Regex, s: &'a str) -> Option<fancy_regex::Captures<'a>> {
    r.captures(s).ok().flatten()
}

pub fn find_all<'a>(r: &'a Regex, s: &'a str) -> Vec<fancy_regex::Captures<'a>> {
    r.captures_iter(s).filter_map(|c| c.ok()).collect()
}

pub fn replace_all(r: &Regex, s: &str, rep: &str) -> String {
    r.replace_all(s, rep).into_owned()
}

pub fn group<'a>(c: &fancy_regex::Captures<'a>, i: usize) -> &'a str {
    c.get(i).map(|m| m.as_str()).unwrap_or("")
}

pub fn named<'a>(c: &fancy_regex::Captures<'a>, n: &str) -> &'a str {
    c.name(n).map(|m| m.as_str()).unwrap_or("")
}

/// Python str.split() (whitespace) joined by single spaces.
pub fn squash_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn char_len(s: &str) -> usize {
    s.chars().count()
}

/// First `n` characters (not bytes).
pub fn take_chars(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// parse.first_sentence: first sentence, squashed, capped at `limit` chars with an ellipsis.
pub fn first_sentence(text: &str, limit: usize) -> String {
    let text = squash_ws(text);
    let mut s = match find(re!(r"^(.+?[.!?])(\s|$)"), &text) {
        Some(c) => group(&c, 1).to_string(),
        None => text.clone(),
    };
    if char_len(&s) > limit {
        let cut = take_chars(&s, limit - 1);
        let cut = match cut.rfind(' ') {
            Some(i) => cut[..i].to_string(),
            None => cut,
        };
        s = cut + "…";
    }
    s.trim_end_matches('.').to_string()
}

/// parse.split_value_list: "yes (the default), no or ask." -> [yes, no, ask]
pub fn split_value_list(chunk: &str) -> Vec<String> {
    let chunk = replace_all(re!(r"\([^)]*\)"), chunk, "");
    let chunk = re!(r"[.;\]\n]|\s{2,}").split(&chunk).next().and_then(|r| r.ok()).unwrap_or("").to_string();
    let parts: Vec<String> = re!(r",|\bor\b|\band\b|\|")
        .split(&chunk)
        .filter_map(|r| r.ok())
        .map(|p| p.trim_matches(|c| c == ' ' || c == '\'' || c == '"' || c == '`').to_string())
        .filter(|p| !p.is_empty())
        .collect();
    if parts.len() >= 2 && parts.iter().all(|p| is_match(re!(r"^[\w.+@-]{1,30}$"), p)) {
        parts
    } else {
        vec![]
    }
}

/// Single-quote for fish.
pub fn fq(s: &str) -> String {
    format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'"))
}

/// Single-quote for POSIX shells.
pub fn sq(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

pub fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"))
}

fn xdg(var: &str, fallback: &str) -> PathBuf {
    match std::env::var_os(var) {
        Some(v) if !v.is_empty() => PathBuf::from(v),
        _ => home().join(fallback),
    }
}

pub fn cache_home() -> PathBuf {
    xdg("XDG_CACHE_HOME", ".cache")
}
pub fn config_home() -> PathBuf {
    xdg("XDG_CONFIG_HOME", ".config")
}

/// ~/.cache/complete-me/<kind>, created on demand.
pub fn cache_dir(kind: &str) -> PathBuf {
    let d = cache_home().join("complete-me").join(kind);
    let _ = std::fs::create_dir_all(&d);
    d
}

pub fn which(cmd: &str) -> Option<PathBuf> {
    if cmd.contains('/') {
        let p = PathBuf::from(cmd);
        return if is_executable(&p) { Some(p) } else { None };
    }
    let path = std::env::var_os("PATH")?;
    for d in std::env::split_paths(&path) {
        let p = d.join(cmd);
        if is_executable(&p) {
            return Some(p);
        }
    }
    None
}

pub fn is_executable(p: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p).map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0).unwrap_or(false)
}

/// Environment for help probing: scrubbed, no pagers, no colour.
pub fn help_env() -> Vec<(String, String)> {
    let mut env: Vec<(String, String)> = std::env::vars()
        .filter(|(k, _)| matches!(k.as_str(), "PATH" | "HOME" | "LANG" | "LC_ALL" | "USER" | "LOGNAME"))
        .collect();
    for (k, v) in [
        ("TERM", "dumb"),
        ("NO_COLOR", "1"),
        ("PAGER", "cat"),
        ("MANPAGER", "cat"),
        ("GIT_PAGER", "cat"),
        ("COLUMNS", "120"),
        ("MANWIDTH", "120"),
        ("CLICOLOR", "0"),
    ] {
        env.push((k.into(), v.into()));
    }
    env
}

/// Run argv with stdin=/dev/null, stdout+stderr captured, in its own process group;
/// on timeout the whole group gets SIGKILL.  Output is capped at 400 kB.
pub fn run_captured(argv: &[String], timeout: Duration, env: Option<&[(String, String)]>) -> String {
    use std::os::unix::process::CommandExt;
    if argv.is_empty() {
        return String::new();
    }
    let mut cmd = Proc::new(&argv[0]);
    cmd.args(&argv[1..]).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).process_group(0);
    if let Some(env) = env {
        cmd.env_clear();
        for (k, v) in env {
            cmd.env(k, v);
        }
    }
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(_) => return String::new(),
    };
    let pid = child.id() as i32;
    // Read both pipes on threads so a chatty child can't fill a pipe and deadlock us.
    let mut so = child.stdout.take().unwrap();
    let mut se = child.stderr.take().unwrap();
    let t1 = std::thread::spawn(move || {
        let mut b = Vec::new();
        let _ = so.by_ref().take(400_000).read_to_end(&mut b);
        b
    });
    let t2 = std::thread::spawn(move || {
        let mut b = Vec::new();
        let _ = se.by_ref().take(400_000).read_to_end(&mut b);
        b
    });
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {
                if start.elapsed() > timeout {
                    unsafe {
                        libc::kill(-pid, libc::SIGKILL);
                    }
                    let _ = child.wait();
                    break;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(_) => break,
        }
    }
    // A grandchild may still hold the pipes open; kill the group so the readers finish.
    unsafe {
        libc::kill(-pid, libc::SIGKILL);
    }
    let mut out = t1.join().unwrap_or_default();
    out.extend(t2.join().unwrap_or_default());
    String::from_utf8_lossy(&out).into_owned()
}

/// Write via a temp file + rename so readers never see half a file.
pub fn atomic_write(path: &std::path::Path, text: &str) -> std::io::Result<()> {
    let tmp = path.with_file_name(format!(".{}.{}.tmp", path.file_name().unwrap().to_string_lossy(), std::process::id()));
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}

pub fn is_tty(fd: i32) -> bool {
    unsafe { libc::isatty(fd) == 1 }
}

/// Split CLI args into (flags/options, positionals) honouring `--`.
/// Returns the positionals; options are read with `opt_value` / `has_flag`.
pub struct Args {
    pub flags: Vec<(String, Option<String>)>,
    pub rest: Vec<String>,
}

impl Args {
    /// `valued` lists option names that take a value (e.g. ["--current", "--flavor"]).
    pub fn parse(args: &[String], valued: &[&str]) -> Args {
        let mut flags = vec![];
        let mut rest = vec![];
        let mut i = 0;
        while i < args.len() {
            let a = &args[i];
            if a == "--" {
                rest.extend(args[i + 1..].iter().cloned());
                break;
            }
            if a.starts_with("--") && a.len() > 2 {
                if let Some((k, v)) = a.split_once('=') {
                    flags.push((k.to_string(), Some(v.to_string())));
                } else if valued.contains(&a.as_str()) {
                    flags.push((a.clone(), args.get(i + 1).cloned()));
                    i += 1;
                } else {
                    flags.push((a.clone(), None));
                }
            } else if a.starts_with('-') && a.len() > 1 && valued.contains(&a.as_str()) {
                flags.push((a.clone(), args.get(i + 1).cloned()));
                i += 1;
            } else if a.starts_with('-') && a.len() > 1 && rest.is_empty() {
                flags.push((a.clone(), None));
            } else {
                rest.push(a.clone());
            }
            i += 1;
        }
        Args { flags, rest }
    }
    pub fn has(&self, name: &str) -> bool {
        self.flags.iter().any(|(k, _)| k == name)
    }
    pub fn value(&self, name: &str) -> Option<String> {
        self.flags.iter().rev().find(|(k, _)| k == name).and_then(|(_, v)| v.clone())
    }
    pub fn values(&self, name: &str) -> Vec<String> {
        self.flags.iter().filter(|(k, _)| k == name).filter_map(|(_, v)| v.clone()).collect()
    }
}

/// Python shlex.quote: safe strings pass through, everything else is single-quoted.
pub fn shquote(s: &str) -> String {
    if s.is_empty() {
        return "''".into();
    }
    if s.chars().all(|c| c.is_alphanumeric() || "_@%+=:,./-".contains(c)) && s.is_ascii() {
        return s.into();
    }
    format!("'{}'", s.replace('\'', "'\"'\"'"))
}

/// Python shlex.split (POSIX mode): quotes, backslashes, no comments.
pub fn shell_words(s: &str) -> Vec<String> {
    let mut out = vec![];
    let mut cur = String::new();
    let mut have = false;
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                have = true;
                for n in chars.by_ref() {
                    if n == '\'' {
                        break;
                    }
                    cur.push(n);
                }
            }
            '"' => {
                have = true;
                while let Some(n) = chars.next() {
                    match n {
                        '"' => break,
                        '\\' => match chars.peek() {
                            Some(&q) if "\\\"$`\n".contains(q) => {
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
                if let Some(n) = chars.next() {
                    cur.push(n);
                }
            }
            c if c.is_whitespace() => {
                if have {
                    out.push(std::mem::take(&mut cur));
                    have = false;
                }
            }
            c => {
                have = true;
                cur.push(c);
            }
        }
    }
    if have {
        out.push(cur);
    }
    out
}
