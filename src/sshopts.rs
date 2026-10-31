//! ssh -oKey=Value: every ssh_config keyword with its meaning and its allowed values.
//! Parsed from ssh_config(5) once and cached; algorithm lists come from `ssh -Q`.

use crate::re;
use crate::util::*;
use indexmap::IndexMap;
use std::time::Duration;

const QUERY: &[(&str, &str)] = &[
    ("ciphers", "cipher"), ("macs", "mac"), ("kexalgorithms", "kex"), ("hostkeyalgorithms", "key-sig"),
    ("pubkeyacceptedalgorithms", "key-sig"), ("hostbasedacceptedalgorithms", "key-sig"), ("casignaturealgorithms", "sig"),
];
const FILES: &[&str] = &[
    "identityfile", "certificatefile", "userknownhostsfile", "globalknownhostsfile", "controlpath", "identityagent",
    "securitykeyprovider", "pkcs11provider", "revokedhostkeys", "include", "knownhostscommand",
];
const HOSTS: &[&str] = &["proxyjump", "hostname", "hostkeyalias"];
const STOP: &[&str] = &["the", "a", "an", "be", "this", "that", "it", "is", "to", "not", "set", "used", "any", "all", "none."];

pub type Keys = IndexMap<String, (String, Vec<String>)>;

pub fn parse_ssh_config_man(text: &str) -> Keys {
    let mut keys = Keys::new();
    let lines: Vec<&str> = text.split('\n').collect();
    let mut i = 0;
    while i < lines.len() {
        let Some(m) = find(re!(r"^ {5}([A-Z][A-Za-z0-9]+)\s*$"), lines[i]) else {
            i += 1;
            continue;
        };
        let name = group(&m, 1).to_string();
        let mut j = i + 1;
        let mut body = vec![];
        while j < lines.len() && (lines[j].trim().is_empty() || lines[j].starts_with("      ")) {
            body.push(lines[j].trim());
            j += 1;
        }
        let full = squash_ws(&body.join(" "));
        if full.is_empty() {
            // several keywords sharing one description
            i = j;
            continue;
        }
        let mut vals: Vec<String> = vec![];
        if let Some(m2) = find(re!(r"(?i)possible values are:?\s*(.+?)\.(?:\s|$)"), &full) {
            vals = split_value_list(&group(&m2, 1).replace(" and ", ", "));
        }
        if vals.is_empty() {
            if let Some(m2) = find(
                re!(r"(?i)(?:argument[s]? (?:to this keyword )?must be|may be one of|valid arguments are|accepted values are)\s*:?\s*(.+?)\.(?:\s|$)"),
                &full,
            ) {
                vals = split_value_list(group(&m2, 1));
            }
        }
        for m3 in find_all(
            re!(r#"(?i)(?:set to|setting this to|set this to|is set to|flag is set to)\s+"?([\w-]+)"?(?:\s+or\s+"?([\w-]+))?"#),
            &full,
        ) {
            for v in [group(&m3, 1), group(&m3, 2)] {
                if !v.is_empty() && !STOP.contains(&v.to_lowercase().as_str()) && !vals.iter().any(|x| x == v) {
                    vals.push(v.to_string());
                }
            }
        }
        if vals.is_empty() && is_match(re!(r"^(Specifies|Indicates) whether"), &full) {
            vals = vec!["yes".into(), "no".into()];
        }
        if vals == ["yes"] {
            vals = vec!["yes".into(), "no".into()];
        }
        keys.insert(name, (first_sentence(&full, 80), vals));
        i = j;
    }
    keys
}

fn cache_file() -> std::path::PathBuf {
    cache_dir("data").join("ssh_config.tsv")
}

fn load(refresh: bool) -> Keys {
    let f = cache_file();
    if !refresh {
        let fresh = std::fs::metadata(&f)
            .and_then(|m| m.modified())
            .map(|t| t.elapsed().map(|e| e.as_secs() < 30 * 86400).unwrap_or(false))
            .unwrap_or(false);
        if fresh {
            if let Ok(text) = std::fs::read_to_string(&f) {
                let mut keys = Keys::new();
                for line in text.lines() {
                    let mut it = line.splitn(3, '\t');
                    let (Some(k), Some(d), Some(v)) = (it.next(), it.next(), it.next()) else { continue };
                    let vals = if v.is_empty() { vec![] } else { v.split(' ').map(String::from).collect() };
                    keys.insert(k.to_string(), (d.to_string(), vals));
                }
                if !keys.is_empty() {
                    return keys;
                }
            }
        }
    }
    let mut keys = parse_ssh_config_man(&crate::probe::man_text("ssh_config"));
    if which("ssh").is_some() {
        for (k, (_, vals)) in keys.iter_mut() {
            if let Some((_, q)) = QUERY.iter().find(|(name, _)| *name == k.to_lowercase()) {
                let out = run_captured(&["ssh".into(), "-Q".into(), q.to_string()], Duration::from_secs(3), None);
                let algos: Vec<String> = out.split_whitespace().map(String::from).collect();
                if !algos.is_empty() {
                    *vals = algos;
                }
            }
        }
    }
    let text: String = keys
        .iter()
        .map(|(k, (d, v))| format!("{k}\t{}\t{}\n", d.replace(['\t', '\n'], " "), v.join(" ")))
        .collect();
    let _ = atomic_write(&f, &text);
    keys
}

fn expanduser(p: &str) -> String {
    match p.strip_prefix('~') {
        Some(rest) if rest.is_empty() || rest.starts_with('/') => format!("{}{rest}", home().display()),
        _ => p.to_string(),
    }
}

/// glob(base + "*"): entries in base's directory whose name starts with base's last component.
fn glob_prefix(base: &str) -> Vec<String> {
    let (dir, stem) = match base.rfind('/') {
        Some(i) => (&base[..=i], &base[i + 1..]),
        None => ("", base),
    };
    let rd = std::fs::read_dir(if dir.is_empty() { "." } else { dir });
    let mut out: Vec<String> = rd
        .map(|rd| {
            rd.flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| n.starts_with(stem) && (!n.starts_with('.') || stem.starts_with('.')))
                .map(|n| format!("{dir}{n}"))
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out
}

fn known_hosts() -> Vec<String> {
    let mut hosts = vec![];
    if let Ok(text) = std::fs::read_to_string(home().join(".ssh/config")) {
        for line in text.lines() {
            if let Some(m) = find(re!(r"(?i)^\s*Host\s+(.+)"), line) {
                hosts.extend(group(&m, 1).split_whitespace().filter(|h| !h.contains('*') && !h.contains('?')).map(String::from));
            }
        }
    }
    hosts
}

pub fn candidates(current: &str, refresh: bool) -> Vec<(String, String)> {
    let keys = load(refresh);
    // fish re-attaches "-o" itself for the -oKey=Value form
    let cur = current.strip_prefix("-o").unwrap_or(current);
    let Some((key, val)) = cur.split_once('=') else {
        let mut all: Vec<(&String, &(String, Vec<String>))> = keys.iter().collect();
        all.sort_by(|a, b| a.0.cmp(b.0));
        return all.into_iter().map(|(k, (d, _))| (format!("{k}="), d.clone())).collect();
    };
    let Some((real, (_, vals))) = keys.iter().find(|(k, _)| k.to_lowercase() == key.to_lowercase()) else {
        return vec![];
    };
    let mut out: Vec<(String, String)> = vals.iter().map(|v| (format!("{real}={v}"), String::new())).collect();
    let lk = real.to_lowercase();
    if FILES.contains(&lk.as_str()) {
        let base = if val.is_empty() { expanduser("~/.ssh/") } else { expanduser(val) };
        let homes = home().display().to_string();
        for p in glob_prefix(&base).into_iter().take(200) {
            let shown = if !val.starts_with('/') { p.replacen(&homes, "~", 1) } else { p.clone() };
            let slash = if std::path::Path::new(&p).is_dir() { "/" } else { "" };
            out.push((format!("{real}={shown}{slash}"), "file".into()));
        }
    }
    if HOSTS.contains(&lk.as_str()) {
        for h in known_hosts() {
            out.push((format!("{real}={h}"), "host from ~/.ssh/config".into()));
        }
    }
    out
}

pub fn run(args: &[String]) -> i32 {
    let a = Args::parse(args, &["--current"]);
    let lines: Vec<String> = candidates(&a.value("--current").unwrap_or_default(), a.has("--refresh"))
        .into_iter()
        .map(|(v, d)| format!("{v}\t{d}"))
        .collect();
    println!("{}", lines.join("\n"));
    0
}
