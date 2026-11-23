//! Bash typed into fish: translate it (built in: src/sh), check it for danger, hand it back.
//!
//! Used by the fish Enter binding for Bash syntax and for simple export assignments.
//! Output protocol (stdout):
//!     RISK<TAB>reason          zero or more lines; any RISK means the shell asks before running
//!     ---FISH---
//!     <translated fish code>
//!
//! `bash2fish --raw` reads bash on stdin and prints only the fish (what `source FILE` and
//! `eval "$(cmd)"` translate through).
//! `bash2fish --export` only accepts simple export assignments and returns 2 when the
//! fish binding should submit the original line.

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

/// Translate standalone scalar exports that fish would otherwise accept. Leave
/// ambiguous fish syntax and statements with evaluation outside this fast path.
fn translate_exports(src: &str) -> Option<String> {
    use crate::sh::ast::{valid_name, Cmd, DeclArg, Part};

    fn passive(parts: &[Part], assigned: &[&str], quoted: bool) -> bool {
        parts.iter().enumerate().all(|(index, part)| match part {
            // Escapes and unquoted glob, brace and colon-separated tilde
            // expansions can have different meanings in the two shells.
            Part::Lit(text) => !text.contains('\\') && (quoted || (!text.contains(['*', '?', '{', '}']) && !text.contains(":~"))),
            Part::Sgl(_) => true,
            Part::Dbl(inner) => passive(inner, assigned, true),
            Part::Param(p) => {
                valid_name(&p.param) && !assigned.contains(&p.param.as_str())
                    && !p.excl && !p.length && !p.names && p.index.is_none()
                    && p.slice.is_none() && p.repl.is_none() && p.exp.is_none()
                    // Fish also accepts a subscript directly after the variable.
                    && !matches!(parts.get(index + 1), Some(Part::Lit(text)) if p.short && text.starts_with('['))
            }
            _ => false,
        })
    }

    let mut file = crate::sh::parse::parse(src).ok()?;
    if file.stmts.is_empty() {
        return None;
    }
    for stmt in &mut file.stmts {
        if stmt.negated || stmt.background || stmt.coprocess || !stmt.redirs.is_empty() {
            return None;
        }
        let Cmd::Decl { variant, args } = &mut stmt.cmd else { return None };
        if variant != "export" {
            return None;
        }
        if matches!(args.first(), Some(DeclArg::Word(word)) if word.as_lit().as_deref() == Some("--")) {
            args.remove(0);
        }
        if args.is_empty() {
            return None;
        }
        let mut assigned = vec![];
        for arg in args {
            let DeclArg::Assign(a) = arg else { return None };
            // Bash expands the RHS of every argument before assigning any of
            // them. Sequential fish sets are equivalent for independent values.
            if a.append || a.naked || a.index.is_some() || a.array.is_some()
                || a.value.as_ref().is_some_and(|v| !passive(&v.parts, &assigned, false))
            {
                return None;
            }
            assigned.push(a.name.as_str());
        }
    }
    crate::sh::fish::translate(&file).ok()
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
    let (exports_only, args) = match args.split_first() {
        Some((flag, remaining)) if flag == "--export" => (true, remaining),
        _ => (false, args),
    };
    let positional: Vec<&String> = args.iter().filter(|a| a.as_str() != "--").collect();
    let src = match positional.first() {
        Some(s) => s.to_string(),
        None => {
            let mut s = String::new();
            let _ = std::io::stdin().read_to_string(&mut s);
            s
        }
    };
    if exports_only {
        return match translate_exports(&src) {
            Some(code) => {
                println!("---FISH---");
                println!("{}", code.trim_end_matches('\n'));
                0
            }
            None => 2,
        };
    }
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

#[cfg(test)]
mod tests {
    use super::translate_exports;

    #[test]
    fn converts_plain_export_assignments() {
        for (input, expected) in [
            (r#"export LABEL="two words" EMPTY="#, "set -gx LABEL 'two words'; set -gx EMPTY ''"),
            (r#"export PATH="$HOME/bin:$PATH""#, r#"set -gx PATH "$HOME"'/bin:'"$PATH""#),
            ("export -- LABEL=value", "set -gx LABEL 'value'"),
            (r#"export LABEL='a;b|c'"#, "set -gx LABEL 'a;b|c'"),
        ] {
            let result = translate_exports(input).unwrap_or_else(|| panic!("no translation for {input:?}"));
            assert_eq!(result.trim_end(), expected, "{input:?}");
        }
    }

    #[test]
    fn leaves_other_export_forms_to_the_shell() {
        for input in [
            "export", "export -p", "export ONLY", "export X+=value", "export X=(a b)",
            "export X=one Y=$X", "export X=$(printf value)", "export X=$items[2]",
            r"export X=a\ b", "export X={a,b}", "export X=~/bin:~/.local/bin", "export X=*",
            "export X=y; echo $X", "export X=y | cat", "export X=y &", "export X=y > file",
            "echo export X=y",
        ] {
            assert_eq!(translate_exports(input), None, "{input:?}");
        }
    }
}
