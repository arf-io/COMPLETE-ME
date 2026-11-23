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
                let out = query_stdout(&["ssh".into(), "-Q".into(), q.to_string()], Duration::from_millis(350));
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
    let cur = current.strip_prefix("-o").unwrap_or(current);
    let Some((key, val)) = cur.split_once('=') else {
        let mut keys = load(refresh);
        // These work even in installations where the client ships without a man page.
        for (key, description) in [
            ("IdentityFile", "Path to a private key"),
            ("CertificateFile", "Path to a user certificate"),
            ("PubkeyAuthentication", "Use public key authentication"),
            ("PubkeyAcceptedAlgorithms", "Algorithms accepted for public key authentication"),
        ] {
            keys.entry(key.into()).or_insert_with(|| (description.into(), vec![]));
        }
        let prefix = cur.to_ascii_lowercase();
        let mut rows: Vec<_> = keys.into_iter().filter(|(k, _)| k.to_ascii_lowercase().starts_with(&prefix))
            .map(|(k, (d, _))| (format!("{k}="), d)).collect();
        rows.sort();
        return rows;
    };
    let lk = key.to_ascii_lowercase();
    let mut out = vec![];
    if FILES.contains(&lk.as_str()) {
        let base = if val.is_empty() { "~/.ssh/" } else { val };
        let mut decoded = String::new();
        let mut chars = base.chars();
        while let Some(c) = chars.next() {
            decoded.push(if c == '\\' { chars.next().unwrap_or(c) } else { c });
        }
        let absolute = expanduser(&decoded);
        // Preserve the spelling of the user-supplied directory, including ~/ and ./.
        let directory = base.rsplit_once('/').map_or("", |(dir, _)| &base[..=dir.len()]);
        for p in glob_prefix(&absolute).into_iter().take(200) {
            let name = p.rsplit('/').next().unwrap_or(&p);
            if name.chars().any(char::is_control) {
                continue;
            }
            let is_dir = std::path::Path::new(&p).is_dir();
            let slash = if is_dir { "/" } else { "" };
            // ssh parses -o a second time. Escape whitespace for ssh as well as the shell.
            let escaped: String = name.chars().flat_map(|c| {
                matches!(c, '\\' | ' ' | '"' | '\'').then_some('\\').into_iter().chain(std::iter::once(c))
            }).collect();
            let shown = format!("{directory}{escaped}");
            out.push((format!("{key}={shown}{slash}"), if is_dir { "directory" } else { "file" }.into()));
        }
    } else if HOSTS.contains(&lk.as_str()) {
        for h in known_hosts().into_iter().filter(|h| h.starts_with(val)) {
            out.push((format!("{key}={h}"), "host from ~/.ssh/config".into()));
        }
    } else {
        let keys = load(refresh);
        let default_vals = match lk.as_str() {
            "pubkeyauthentication" | "passwordauthentication" => &["yes", "no"][..],
            "stricthostkeychecking" => &["accept-new", "ask", "no", "off", "yes"][..],
            _ => &[],
        };
        let vals = keys.iter().find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, (_, vals))| vals.iter().map(String::as_str).collect::<Vec<_>>())
            .filter(|vals| !vals.is_empty()).unwrap_or_else(|| default_vals.to_vec());
        out.extend(vals.into_iter().filter(|v| v.starts_with(val)).map(|v| (format!("{key}={v}"), String::new())));
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
