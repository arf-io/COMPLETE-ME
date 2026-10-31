//! Bash typed into fish: translate it with babelfish, check it for danger, hand it back.
//!
//! Used by the fish Enter binding: only lines that are NOT valid fish but ARE valid bash get here.
//! Output protocol (stdout):
//!     RISK<TAB>reason          zero or more lines; any RISK means the shell asks before running
//!     ---FISH---
//!     <translated fish code>

use crate::re;
use crate::util::*;
use std::io::{Read, Write};
use std::time::{Duration, Instant};

/// (pattern, reason). Checked against the bash source.
pub const RISKS: &[(&str, &str)] = &[
    ("\\brm\\s+(-[a-zA-Z]*[rRf][a-zA-Z]*\\s+)+", "rm -r/-f: deletes without asking"),
    ("\\brm\\b[^|;&\\n]*\\s(/|~|\\$HOME|/\\*|\\*)(\\s|$|;|&)", "rm on / , ~ or a bare *"),
    ("\\b(curl|wget|fetch)\\b[^|\\n]*\\|\\s*(sudo\\s+)?(ba|z|da|k)?sh\\b", "downloads a script and runs it"),
    ("\\b(sudo|doas|pkexec|su)\\b", "runs as root"),
    ("\\bdd\\b[^\\n]*\\bof=", "dd writes raw data to a file or disk"),
    ("\\b(mkfs(\\.\\w+)?|wipefs|fdisk|sfdisk|parted|gdisk|shred|blkdiscard)\\b", "formats, wipes or repartitions disks"),
    (">\\s*/dev/(sd|nvme|hd|vd|mmcblk|disk)", "writes straight to a disk device"),
    (":\\(\\)\\s*\\{\\s*:\\s*\\|\\s*:\\s*&\\s*\\}\\s*;?\\s*:", "fork bomb"),
    ("\\bchmod\\s+(-[a-zA-Z]*R[a-zA-Z]*\\s+)?[0-7]*7[0-7]{2}\\s+/", "makes system paths world-writable"),
    ("\\bchown\\s+-[a-zA-Z]*R\\b[^\\n]*\\s/(\\s|$)", "recursively re-owns /"),
    ("(^|[^>])>\\s*~?/?[\\w./-]*\\.(bashrc|zshrc|profile|bash_profile)\\b", "overwrites a shell startup file"),
    (">\\s*/etc/", "overwrites a file in /etc"),
    ("\\b(shutdown|reboot|poweroff|halt)\\b", "shuts down or reboots"),
    ("\\bkill\\s+-9\\s+-1\\b|\\bkillall\\b|\\bpkill\\b", "kills processes broadly"),
    ("\\bgit\\s+push\\b[^\\n]*(--force|-f\\b)", "force-pushes"),
    ("\\bgit\\s+(reset\\s+--hard|clean\\s+-[a-zA-Z]*f)", "discards uncommitted work"),
    ("\\beval\\b", "eval runs text as code"),
    ("\\bhistory\\s+-c\\b", "clears your history"),
];

pub fn risks(src: &str) -> Vec<String> {
    let mut found: Vec<String> = vec![];
    for (pat, why) in RISKS {
        if let Ok(r) = fancy_regex::Regex::new(pat) {
            if is_match(&r, src) && !found.iter().any(|f| f == why) {
                found.push(why.to_string());
            }
        }
    }
    found
}

/// Things babelfish leaves as bash-isms: {1..10} brace ranges -> $(seq 1 10).
pub fn preprocess(src: &str) -> String {
    let r = re!(r#"(?<![\w$'"]){(-?\d+)\.\.(-?\d+)(?:\.\.(-?\d+))?}"#);
    let mut out = String::new();
    let mut last = 0;
    for m in find_all(r, src) {
        let whole = m.get(0).unwrap();
        out += &src[last..whole.start()];
        let (a, b, step) = (group(&m, 1), group(&m, 2), group(&m, 3));
        if !step.is_empty() {
            out += &format!("$(seq {a} {step} {b})");
        } else {
            let down = b.parse::<i64>().unwrap_or(0) < a.parse::<i64>().unwrap_or(0);
            out += &format!("$(seq {a} {}{b})", if down { "-1 " } else { "" });
        }
        last = whole.end();
    }
    out += &src[last..];
    out
}

/// Run argv with `input` on stdin; returns (status, stdout, stderr) or None on spawn failure/timeout.
pub fn run_stdin(argv: &[&str], input: &str, timeout: Duration) -> Option<(i32, String, String)> {
    use std::os::unix::process::CommandExt;
    let mut child = std::process::Command::new(argv[0])
        .args(&argv[1..])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .process_group(0)
        .spawn()
        .ok()?;
    let pid = child.id() as i32;
    let mut si = child.stdin.take()?;
    let data = input.to_string();
    let w = std::thread::spawn(move || {
        let _ = si.write_all(data.as_bytes());
    });
    let mut so = child.stdout.take()?;
    let mut se = child.stderr.take()?;
    let r1 = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = so.read_to_string(&mut s);
        s
    });
    let r2 = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = se.read_to_string(&mut s);
        s
    });
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Some(s),
            Ok(None) if start.elapsed() > timeout => {
                unsafe { libc::kill(-pid, libc::SIGKILL) };
                let _ = child.wait();
                break None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(3)),
            Err(_) => break None,
        }
    };
    let _ = w.join();
    let (o, e) = (r1.join().unwrap_or_default(), r2.join().unwrap_or_default());
    status.map(|s| (s.code().unwrap_or(1), o, e))
}

pub fn translate(src: &str) -> Result<String, String> {
    let Some(exe) = which("babelfish") else {
        return Err("babelfish is not installed (brew install babelfish, or: go install github.com/bouk/babelfish@latest)".into());
    };
    let exe = exe.to_string_lossy().into_owned();
    match run_stdin(&[&exe], &preprocess(src), Duration::from_secs(5)) {
        None => Err("babelfish failed: timed out".into()),
        Some((code, out, err)) => {
            if code != 0 || out.trim().is_empty() {
                let e = err.trim();
                return Err(if e.is_empty() { "babelfish could not translate this".into() } else { e.lines().next().unwrap_or("").to_string() });
            }
            Ok(out.trim_end_matches('\n').to_string())
        }
    }
}

pub fn is_bash(src: &str) -> bool {
    which("bash").is_some() && matches!(run_stdin(&["bash", "-n"], src, Duration::from_secs(3)), Some((0, _, _)))
}

pub fn run(args: &[String]) -> i32 {
    let positional: Vec<&String> = args.iter().filter(|a| a.as_str() != "--").collect();
    let src = match positional.first() {
        Some(s) => s.to_string(),
        None => {
            let mut s = String::new();
            let _ = std::io::stdin().read_to_string(&mut s);
            s
        }
    };
    if !is_bash(&src) {
        eprintln!("not valid bash either");
        return 2;
    }
    let fish = translate(&src);
    for r in risks(&src) {
        println!("RISK\t{r}");
    }
    match fish {
        Err(e) => {
            eprintln!("{e}");
            1
        }
        Ok(code) => {
            println!("---FISH---");
            println!("{code}");
            0
        }
    }
}
