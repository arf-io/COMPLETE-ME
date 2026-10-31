//! Bash typed into fish: translate it (built in: src/sh), check it for danger, hand it back.
//!
//! Used by the fish Enter binding: only lines that are NOT valid fish but ARE valid bash get here.
//! Output protocol (stdout):
//!     RISK<TAB>reason          zero or more lines; any RISK means the shell asks before running
//!     ---FISH---
//!     <translated fish code>
//!
//! `bash2fish --raw` reads bash on stdin and prints only the fish (what `source FILE` and
//! `eval "$(cmd)"` translate through).

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

/// bash → fish, with the built-in translator (src/sh). Anything it can't translate faithfully
/// is an error, never a guess.
pub fn translate(src: &str) -> Result<String, String> {
    crate::sh::to_fish(src).map(|s| s.trim_end_matches('\n').to_string())
}

pub fn is_bash(src: &str) -> bool {
    which("bash").is_some() && matches!(run_stdin(&["bash", "-n"], src, Duration::from_secs(3)), Some((0, _, _)))
}

pub fn run(args: &[String]) -> i32 {
    if args.first().map(|s| s.as_str()) == Some("--raw") {
        let mut s = String::new();
        let _ = std::io::stdin().read_to_string(&mut s);
        return match translate(&s) {
            Ok(code) => {
                println!("{code}");
                0
            }
            Err(e) => {
                eprintln!("complete-me bash2fish: {e}");
                1
            }
        };
    }
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
