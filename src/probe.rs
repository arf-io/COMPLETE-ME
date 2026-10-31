//! Gather help text for a command without letting it do anything.
//!
//! Trust order: man page (pure text) > static script reading > `cmd [sub] --help`.
//! `--help` only ever runs inside a bubblewrap sandbox: read-only filesystem, no network,
//! private /tmp and /run (so no D-Bus, agent or docker sockets), killed with us.
//! Without bwrap, binaries get `--help` only if they are not on the deny list, and scripts
//! are read, never run. The target is never run bare, and never with -h or -?.

use crate::model::Command;
use crate::parse::{clean, parse_help};
use crate::re;
use crate::scriptscan;
use crate::util::*;
use std::collections::BTreeSet;
use std::time::{Duration, Instant};

const NEVER_RUN: &[&str] = &[
    "shutdown", "reboot", "poweroff", "halt", "init", "telinit", "kill", "killall", "pkill", "rm", "dd", "shred",
    "wipefs", "fdisk", "sfdisk", "parted", "mkfs", "mkswap", "sudo", "doas", "su", "pkexec", "login", "passwd",
    "yes", "cat", "tee", "sh", "bash", "zsh", "fish", "python", "python3", "node", "perl", "ruby", "vim", "nvim",
    "emacs", "nano", "less", "more", "top", "htop",
];
const MAX_SUBCOMMANDS: usize = 80;
const MAX_DEPTH: usize = 3;

pub fn safe_name(name: &str) -> bool {
    is_match(re!(r"^[A-Za-z0-9_][\w.+-]*$"), name)
}

pub fn sandbox_prefix() -> Option<Vec<String>> {
    if std::env::var("COMPLETE_ME_SANDBOX").as_deref() == Ok("off") {
        return None;
    }
    let bwrap = which("bwrap")?;
    let mut v = vec![bwrap.to_string_lossy().into_owned()];
    // Read-only blocks writes but not connect() on Unix sockets, so the socket directories
    // (/run: session D-Bus, systemd, docker.sock, agents; /tmp: X11, tmux) get private tmpfs.
    for a in [
        "--ro-bind", "/", "/", "--dev", "/dev", "--proc", "/proc", "--tmpfs", "/tmp", "--tmpfs", "/run",
        "--unshare-all", "--die-with-parent", "--new-session", "--",
    ] {
        v.push(a.into());
    }
    Some(v)
}

fn run(argv: &[String], secs: f64) -> String {
    run_captured(argv, Duration::from_secs_f64(secs), Some(&help_env()))
}

pub fn is_script(path: &std::path::Path) -> bool {
    use std::io::Read;
    let mut b = [0u8; 2];
    std::fs::File::open(path).and_then(|mut f| f.read_exact(&mut b)).map(|_| &b == b"#!").unwrap_or(false)
}

pub fn man_text(name: &str) -> String {
    man_text_timeout(name, 4.0)
}

pub fn man_text_timeout(name: &str, secs: f64) -> String {
    if which("man").is_none() || !safe_name(name) {
        return String::new();
    }
    let head = |s: &str| take_chars(s, 200);
    let mut out = run(&["man".into(), "--nh".into(), "--nj".into(), "-P".into(), "cat".into(), name.into()], secs);
    if out.trim().is_empty() || head(&out).contains("No manual entry") || head(&out).contains("unrecognized option") {
        out = run(&["man".into(), "-P".into(), "cat".into(), name.into()], secs); // BSD/macOS man
    }
    if head(&out).contains("No manual entry") {
        return String::new();
    }
    clean(&out)
}

pub fn help_text(argv: &[String], secs: f64) -> String {
    let mut full: Vec<String> = match sandbox_prefix() {
        Some(p) => p,
        None => {
            let base = std::path::Path::new(&argv[0]).file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            if NEVER_RUN.contains(&base.as_str()) || is_script(std::path::Path::new(&argv[0])) {
                return String::new();
            }
            vec![]
        }
    };
    full.extend(argv.iter().cloned());
    full.push("--help".into());
    run(&full, secs)
}

pub fn looks_like_help(text: &str) -> bool {
    is_match(re!(r"(?im)usage|options|commands|^\s+-"), text)
}

/// Many CLIs print the top-level help for `cmd sub --help`; that tells us nothing about sub.
pub fn echoes_parent(child: &Command, parent: &Command) -> bool {
    let ps: BTreeSet<&String> = parent.subcommands.keys().collect();
    let cs: BTreeSet<&String> = child.subcommands.keys().collect();
    if !cs.is_empty() && !ps.is_empty() {
        let common = cs.intersection(&ps).count() as f64;
        if common >= f64::max(2.0, 0.8 * ps.len() as f64) {
            return true;
        }
    }
    let pf: BTreeSet<String> = parent.options.iter().flat_map(|o| o.flags()).collect();
    let cf: BTreeSet<String> = child.options.iter().flat_map(|o| o.flags()).collect();
    !cf.is_empty() && cf.intersection(&pf).count() as f64 >= 0.8 * cf.len() as f64 && cf.len() >= 4 && cs.is_empty()
}

/// git/clevis style: executables named NAME-sub[-subsub] on PATH are subcommands.
pub fn path_dispatch(name: &str, cmd: &mut Command) -> bool {
    let prefix = format!("{name}-");
    let mut names = BTreeSet::new();
    if let Some(path) = std::env::var_os("PATH") {
        for d in std::env::split_paths(&path) {
            let Ok(rd) = std::fs::read_dir(&d) else { continue };
            for e in rd.flatten() {
                let n = e.file_name().to_string_lossy().into_owned();
                if n.starts_with(&prefix) && safe_name(&n) && is_executable(&e.path()) {
                    names.insert(n[prefix.len()..].to_string());
                }
            }
        }
    }
    let seen = !names.is_empty();
    for rest in names.iter().take(300) {
        let mut node = &mut *cmd;
        for part in rest.split('-') {
            if part.is_empty() {
                break;
            }
            if !node.subcommands.contains_key(part) {
                node.subcommands.insert(part.to_string(), Command::with_source(part, "path"));
            }
            node = node.subcommands.get_mut(part).unwrap();
        }
    }
    if seen && !cmd.sources.iter().any(|s| s == "path") {
        cmd.sources.push("path".into());
    }
    seen
}

fn probe_level(path: &str, parent: &mut Command, prefix: Vec<String>, parent_text: &str, started: Instant, budget: f64, depth: usize) {
    let subs: Vec<String> = parent.subcommands.keys().take(MAX_SUBCOMMANDS).cloned().collect();
    // 8 at a time, like the Python thread pool.
    let mut results: Vec<(String, String)> = vec![];
    for chunk in subs.chunks(8) {
        let handles: Vec<_> = chunk
            .iter()
            .map(|sub| {
                let mut argv = vec![path.to_string()];
                argv.extend(prefix.iter().cloned());
                argv.push(sub.clone());
                let sub = sub.clone();
                let over = started.elapsed().as_secs_f64() > budget;
                std::thread::spawn(move || {
                    let text = if over { String::new() } else { help_text(&argv, 3.0) };
                    (sub, text)
                })
            })
            .collect();
        for h in handles {
            if let Ok(r) = h.join() {
                results.push(r);
            }
        }
    }
    for (sub, text) in results {
        if text.is_empty() || !looks_like_help(&text) || text == parent_text {
            continue;
        }
        let mut parsed = parse_help(&text, &sub, "help");
        parsed.subcommands.shift_remove(&sub);
        if echoes_parent(&parsed, parent) {
            continue;
        }
        let node = parent.subcommands.get_mut(&sub).unwrap();
        if node.options.is_empty() {
            node.options = parsed.options;
        }
        if node.desc.is_empty() {
            node.desc = parsed.desc;
        }
        let parsed_has_subs = !parsed.subcommands.is_empty();
        for (n, s) in parsed.subcommands {
            node.subcommands.entry(n).or_insert(s);
        }
        if parsed_has_subs {
            node.takes_files = parsed.takes_files;
        }
        if !node.subcommands.is_empty() && depth < MAX_DEPTH {
            let mut p = prefix.clone();
            p.push(sub.clone());
            probe_level(path, node, p, &text, started, budget, depth + 1);
        }
    }
}

/// quick: TAB-time pass. Top level only, and the whole thing is held to about a second.
pub fn gather(name: &str, quick: bool) -> Option<Command> {
    if !safe_name(name) {
        return None;
    }
    let path = which(name)?;
    let path_s = path.to_string_lossy().into_owned();
    let started = Instant::now();
    let mut cmd = Command::new(name);
    let mut found = false;

    if is_script(&path) {
        if let Some(s) = scriptscan::scan(&path, name) {
            cmd.merge(s);
            found = true;
        }
    }
    // Quick mode runs --help and man at the same time so TAB never waits for both in a row.
    let (help_secs, man_secs) = if quick { (1.0, 1.0) } else { (4.0, 4.0) };
    let p2 = path_s.clone();
    let help_thread = std::thread::spawn(move || help_text(&[p2], help_secs));
    let n2 = name.to_string();
    let man_thread = std::thread::spawn(move || man_text_timeout(&n2, man_secs));
    let h = help_thread.join().unwrap_or_default();
    let m = man_thread.join().unwrap_or_default();
    if !h.is_empty() && looks_like_help(&h) {
        cmd.merge(parse_help(&h, name, "help"));
        found = true;
    }
    if !m.is_empty() {
        cmd.merge(parse_help(&m, name, "man"));
        found = true;
    }
    found = path_dispatch(name, &mut cmd) || found;
    if !found {
        return None;
    }
    if !quick && sandbox_prefix().is_some() && !cmd.subcommands.is_empty() {
        probe_level(&path_s, &mut cmd, vec![], &h, started, 25.0, 1);
    }
    Some(cmd)
}
