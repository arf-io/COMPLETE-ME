//! Git config grammar and scoped, read-only lookups for completion.
use super::{arguments, query, Rows};

const SUBCOMMANDS: &[(&str, &str)] = &[
    ("list", "List settings"), ("get", "Read a setting"), ("set", "Write a setting"),
    ("unset", "Remove a setting"), ("rename-section", "Rename a section"),
    ("remove-section", "Remove a section"), ("edit", "Open the config file"),
];
const SCOPES: &[(&str, &str)] = &[
    ("--global", "User config"), ("--local", "Repository config"),
    ("--worktree", "Worktree config"), ("--system", "System config"),
    ("--file", "An explicit config file"), ("--blob", "Read a config blob"),
];
const DISPLAY: &[(&str, &str)] = &[
    ("--show-origin", "Show where each setting comes from"), ("--show-scope", "Show each configuration scope"),
    ("--includes", "Read included config files"), ("--null", "NUL-terminate results"),
    ("--name-only", "Show keys only"),
];
const LEGACY: &[(&str, &str)] = &[
    ("--get", "Read a setting"), ("--get-all", "Read all values"),
    ("--get-regexp", "Read keys matching a pattern"), ("--get-urlmatch", "Read URL-specific settings"),
    ("--list", "List settings"), ("--add", "Append a value"), ("--replace-all", "Replace all matching values"),
    ("--unset", "Remove a setting"), ("--unset-all", "Remove all matching values"),
    ("--rename-section", "Rename a section"), ("--remove-section", "Remove a section"), ("--edit", "Open the config file"),
];
const TYPES: &[&str] = &["bool", "int", "bool-or-int", "bool-or-str", "path", "expiry-date", "color"];
const EDITABLE: &[&str] = &[
    "user.email", "user.name", "init.defaultBranch",
    "core.autocrlf", "core.eol", "core.filemode", "core.ignorecase", "core.precomposeUnicode",
    "commit.gpgSign", "tag.gpgSign", "pull.rebase", "pull.ff", "push.default", "push.followTags",
    "push.autoSetupRemote", "fetch.prune", "fetch.pruneTags", "merge.ff", "merge.conflictStyle",
    "rebase.autoStash", "rebase.autoSquash", "diff.algorithm", "diff.renames", "color.ui",
    "status.showUntrackedFiles", "rerere.enabled", "rerere.autoupdate", "remote.pushDefault",
    "branch.autoSetupMerge", "branch.autoSetupRebase", "submodule.recurse",
];

fn preference(key: &str) -> bool {
    if EDITABLE.iter().any(|name| name.eq_ignore_ascii_case(key)) {
        return true;
    }
    if !safe_key(key) {
        return false;
    }
    let key = key.to_ascii_lowercase();
    let (section, field) = key.rsplit_once('.').unwrap_or_default();
    section.starts_with("branch.") && matches!(field, "merge" | "remote" | "pushremote" | "rebase")
}

fn option_rows(options: &[(&str, &str)]) -> Rows {
    options.iter().map(|(v, d)| ((*v).into(), (*d).into())).collect()
}

fn modern() -> bool {
    let version = query("git", &[], &["--version"]);
    let mut fields = version.split_whitespace().nth(2).unwrap_or("").split('.');
    let major = fields.next().and_then(|s| s.parse::<u32>().ok()).unwrap_or(0);
    let minor = fields.next().and_then(|s| s.parse::<u32>().ok()).unwrap_or(0);
    (major, minor) >= (2, 46)
}

fn safe_key(key: &str) -> bool {
    key.split_once('.').is_some_and(|(section, tail)| !section.is_empty() && !tail.is_empty())
        && key.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-/".contains(&b))
}

/// Accept literal file and scope selectors while excluding match expressions,
/// aliases, and commands from every query.
fn config_scope(args: &super::Arguments<'_>, default_local: bool) -> Option<Vec<String>> {
    let mut scopes: Vec<String> = args.switches.iter()
        .filter(|word| SCOPES[..4].iter().any(|(name, _)| *word == name))
        .map(|word| (*word).into()).collect();
    for (flag, value) in &args.values {
        let flag = if *flag == "-f" || *flag == "f" { "--file" } else { flag };
        if matches!(flag, "--file" | "--blob") {
            if value.is_empty() || *value == "-" || value.starts_with('~')
                || value.contains(['$', '\x60', '\n', '\r'])
            {
                return None;
            }
            scopes.push(format!("{flag}={value}"));
        }
    }
    if scopes.len() > 1 {
        return None;
    }
    if scopes.is_empty() && default_local {
        scopes.push("--local".into());
    }
    Some(scopes)
}

fn keys(git_scope: &[String], config_scope: &[String], current: &str, sections: bool) -> Rows {
    let mut command = vec!["--no-pager", "config", "--null", "--name-only", "--list"];
    command.extend(config_scope.iter().map(String::as_str));
    let recent = query("git", git_scope, &command);
    let mut rows: Rows = recent.split_terminator('\0')
        .filter(|key| safe_key(key))
        .filter_map(|key| {
            let value = if sections { key.rsplit_once('.')?.0 } else { key };
            Some((value.into(), if sections { "Configured section" } else { "Configured key" }.into()))
        }).collect();
    if sections {
        return rows;
    }
    rows.extend(EDITABLE.iter().map(|name| ((*name).into(), "Git configuration key".into())));
    if !current.is_empty() {
        rows.extend(query("git", &[], &["--no-pager", "help", "--config"])
            .lines().filter(|key| safe_key(key))
            .map(|key| (key.into(), "Git configuration key".into())));
    }
    rows
}

fn value(git_scope: &[String], config_scope: &[String], key: &str) -> Rows {
    // Arbitrary Git config values may contain credentials or shell commands.
    // Prefill explicitly selected scalar preferences only.
    if !preference(key)
        || config_scope.iter().any(|flag| flag.starts_with("--blob="))
    {
        return vec![];
    }
    let mut command = vec!["--no-pager", "config", "--null", "--get"];
    command.extend(config_scope.iter().map(String::as_str));
    command.extend(["--", key]);
    query("git", git_scope, &command)
        .strip_suffix('\0')
        .filter(|v| !v.starts_with('-'))
        .filter(|value| {
            let key = key.to_ascii_lowercase();
            let ref_or_remote = key == "remote.pushdefault" || key.starts_with("branch.")
                && key.rsplit_once('.').is_some_and(|(_, field)| matches!(field, "merge" | "remote" | "pushremote"));
            // These settings name refs and configured remotes. Leave URL-like
            // strings and atypical command text out of the editor.
            !ref_or_remote || value.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-/".contains(&b))
        })
        .map(|v| vec![(v.into(), "Current value in selected config".into())])
        .unwrap_or_default()
}

fn parsed(words: &[String]) -> (Option<&'static str>, super::Arguments<'_>, Option<&'static str>) {
    let sub = words.first().and_then(|word| SUBCOMMANDS.iter().find(|(name, _)| word == name))
        .map(|(name, _)| *name);
    let rest = if sub.is_some() { &words[1..] } else { words };
    let args = arguments(rest, takes);
    let legacy = args.switches.iter()
        .find_map(|word| LEGACY.iter().find(|(flag, _)| word == flag).map(|(flag, _)| *flag));
    (sub, args, legacy)
}

fn takes(flag: &str) -> bool {
    matches!(flag, "-f" | "f" | "--file" | "--blob" | "-t" | "t" | "--type" | "--value" | "--comment" | "--default" | "--url")
}

fn set_command(sub: Option<&str>, legacy: Option<&str>) -> bool {
    sub == Some("set") || sub.is_none() && matches!(legacy, None | Some("--replace-all"))
}

pub(super) fn value_slot(words: &[String], current: &str) -> bool {
    let (sub, args, legacy) = parsed(words);
    set_command(sub, legacy) && args.positional.len() == 1 && args.pending.is_none()
        && (!args.options || !current.starts_with('-'))
        && !args.switches.iter().any(|word| matches!(*word, "-e" | "-l"))
}

pub(super) fn candidates(git_scope: &[String], words: &[String], current: &str) -> Rows {
    let (sub, args, legacy) = parsed(words);
    let pos = &args.positional;
    if let Some((flag, offset)) = args.pending.map(|flag| (flag, 0))
        .or_else(|| crate::context::value_option(current, takes)
            .filter(|(_, offset)| *offset < current.len() || current.ends_with('=')))
    {
        return if matches!(flag, "-t" | "t" | "--type") {
            TYPES.iter().map(|value| (format!("{}{value}", &current[..offset]), "Git config value type".into())).collect()
        } else { vec![] };
    }
    if args.options && current.starts_with('-') {
        let mut rows = option_rows(SCOPES);
        if sub.is_none() {
            rows.extend(option_rows(LEGACY));
        }
        if matches!(sub, Some("set" | "unset" | "get")) {
            rows.extend(option_rows(&[("--all", "Allow multiple values"), ("--value", "Filter existing values"), ("--fixed-value", "Use literal value matching")]));
        }
        if matches!(sub, Some("get" | "list")) {
            rows.extend(option_rows(DISPLAY));
        }
        if sub == Some("get") {
            rows.extend(option_rows(&[("--regexp", "Match keys with a pattern"), ("--default", "Value if the key is missing"), ("--url", "Match URL-specific settings")]));
        }
        if sub == Some("set") {
            rows.extend(option_rows(&[("--comment", "Add a comment to the setting")]));
        }
        if matches!(sub, None | Some("set" | "get")) {
            rows.extend(option_rows(&[("--type", "Interpret a value as a specific type"), ("--bool", "Boolean"), ("--int", "Integer"), ("--path", "Path")]));
        }
        return rows;
    }

    if words.is_empty() && !current.contains('.') && modern() {
        let rows: Rows = option_rows(SUBCOMMANDS).into_iter()
            .filter(|(v, _)| v.starts_with(current)).collect();
        if !rows.is_empty() {
            return rows;
        }
    }

    let sections = matches!(sub, Some("remove-section" | "rename-section"))
        || matches!(legacy, Some("--remove-section" | "--rename-section"));
    let set = set_command(sub, legacy);
    let read_key = !sections && match sub {
        Some("get" | "set" | "unset") => true,
        None => !matches!(legacy, Some("--list" | "--get-regexp" | "--edit")),
        _ => false,
    };
    let old_edit = args.switches.iter().any(|word| matches!(*word, "-e" | "-l"));
    let has_filter = args.values.iter().any(|(flag, _)| *flag == "--value")
        || args.switches.iter().any(|word| matches!(*word, "--append" | "--regexp") || word.starts_with("--regexp="));
    // An unscoped write updates local config, whereas a read can see all scopes.
    let write = matches!(sub, Some("set" | "unset" | "remove-section" | "rename-section"))
        || matches!(legacy, Some("--replace-all" | "--add" | "--unset" | "--unset-all" | "--remove-section" | "--rename-section"));
    let default_local = write || set && pos.len() == 1;
    let Some(config_scope) = config_scope(&args, default_local) else { return vec![] };
    if old_edit {
        return vec![];
    }
    match pos.as_slice() {
        [] if read_key || sections => keys(git_scope, &config_scope, current, sections),
        [key] if set && !has_filter =>
            value(git_scope, &config_scope, key),
        _ => vec![],
    }
}
