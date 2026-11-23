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

/// Read only regular files, even when the path points to a device or FIFO.
pub(crate) fn open_regular(path: impl AsRef<std::path::Path>) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    let file = std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NONBLOCK).open(path)?;
    if !file.metadata()?.is_file() {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "expected a regular file"));
    }
    Ok(file)
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
    capture(argv, timeout, env, &[], true, None).0
}

/// Capture stdout from a fixed read-only query. Failed queries have no candidates.
pub fn query_stdout(argv: &[String], timeout: Duration) -> String {
    query_result(argv, timeout).0
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueryStatus {
    Success,
    Missing,
    Timeout,
    Failed(Option<i32>),
}

pub fn query_result(argv: &[String], timeout: Duration) -> (String, QueryStatus) {
    query_result_scoped(argv, timeout, &[])
}

pub(crate) fn query_result_scoped(
    argv: &[String], timeout: Duration, overrides: &[(String, Option<String>)],
) -> (String, QueryStatus) {
    capture(argv, timeout, None, overrides, false, None)
}

pub(crate) fn query_with_stdin(argv: &[String], timeout: Duration, input: Vec<u8>) -> (String, QueryStatus) {
    capture(argv, timeout, None, &[], false, Some(input))
}

fn capture(argv: &[String], timeout: Duration, env: Option<&[(String, String)]>, overrides: &[(String, Option<String>)], include_errors: bool, input: Option<Vec<u8>>) -> (String, QueryStatus) {
    use std::io::{ErrorKind, Write};
    use std::os::fd::AsRawFd;
    use std::os::unix::process::CommandExt;
    fn nonblocking(pipe: &impl AsRawFd) -> bool {
        let fd = pipe.as_raw_fd();
        // SAFETY: the borrowed pipe owns a valid descriptor throughout both calls.
        unsafe {
            let flags = libc::fcntl(fd, libc::F_GETFL);
            flags >= 0 && libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) == 0
        }
    }
    fn drain(pipe: &mut Option<impl Read>, output: &mut Vec<u8>) {
        const LIMIT: usize = 400_000;
        let mut buffer = [0; 8192];
        while let Some(reader) = pipe {
            let len = buffer.len().min(LIMIT - output.len());
            match reader.read(&mut buffer[..len]) {
                Ok(0) => { *pipe = None; }
                Ok(len) => output.extend_from_slice(&buffer[..len]),
                Err(e) if e.kind() == ErrorKind::WouldBlock => return,
                Err(_) => { *pipe = None; }
            }
        }
    }
    if argv.is_empty() {
        return (String::new(), QueryStatus::Missing);
    }
    let mut cmd = Proc::new(&argv[0]);
    cmd.args(&argv[1..])
        .stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped()).stderr(Stdio::piped()).process_group(0);
    if let Some(env) = env {
        cmd.env_clear();
        for (k, v) in env {
            cmd.env(k, v);
        }
    }
    for (name, value) in overrides {
        if let Some(value) = value {
            cmd.env(name, value);
        } else {
            cmd.env_remove(name);
        }
    }
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (String::new(), QueryStatus::Missing),
        Err(_) => return (String::new(), QueryStatus::Failed(None)),
    };
    let pid = child.id() as i32;
    let mut stdin = child.stdin.take();
    let mut stdout = child.stdout.take();
    let mut stderr = child.stderr.take();
    let mut remaining = input.as_deref().unwrap_or_default();
    let mut out = Vec::new();
    let mut err = Vec::new();
    let start = Instant::now();
    let ready = stdin.as_ref().is_none_or(nonblocking)
        && stdout.as_ref().is_none_or(nonblocking)
        && stderr.as_ref().is_none_or(nonblocking);
    let status = if !ready { QueryStatus::Failed(None) } else { loop {
        drain(&mut stdout, &mut out);
        drain(&mut stderr, &mut err);
        if let Some(writer) = stdin.as_mut() {
            match writer.write(remaining) {
                Ok(n) => { remaining = &remaining[n..]; }
                Err(e) if e.kind() == ErrorKind::WouldBlock => {}
                Err(_) => { stdin = None; }
            }
            if remaining.is_empty() { stdin = None; }
        }
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break QueryStatus::Success,
            Ok(Some(status)) => break QueryStatus::Failed(status.code()),
            Ok(None) => {
                let left = timeout.saturating_sub(start.elapsed());
                if left.is_zero() { break QueryStatus::Timeout; }
                std::thread::sleep(left.min(Duration::from_millis(5)));
            }
            Err(_) => break QueryStatus::Failed(None),
        }
    }};
    // Close the parent pipes without waiting for descendants to release their copies.
    unsafe {
        libc::kill(-pid, libc::SIGKILL);
    }
    if ready {
        drain(&mut stdout, &mut out);
        drain(&mut stderr, &mut err);
    }
    if child.try_wait().ok().flatten().is_none() {
        std::thread::spawn(move || { let _ = child.wait(); });
    }
    if include_errors {
        out.extend(err);
    } else if status != QueryStatus::Success {
        out.clear();
    }
    (String::from_utf8_lossy(&out).into_owned(), status)
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

/// Read the two NUL-terminated editor fields from the shell's stdin stream.
pub(crate) fn editor_input(input: &mut impl std::io::BufRead) -> Option<(String, String)> {
    fn field(input: &mut impl std::io::BufRead) -> Option<String> {
        use std::io::BufRead;
        let mut bytes = Vec::new();
        input.take(128 * 1024).read_until(0, &mut bytes).ok()?;
        (bytes.pop() == Some(0)).then(|| String::from_utf8(bytes).ok()).flatten()
    }
    Some((field(input)?, field(input)?))
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
