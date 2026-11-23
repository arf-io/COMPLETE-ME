//! Repository inventories and Git argument positions.
use super::{arguments, global, metadata, query, value_option, Rows};

const GLOBAL: &[&str] = &["-C", "-c", "--git-dir", "--work-tree", "--namespace", "--config-env"];
const FORWARD: &[&str] = &["-C", "--git-dir", "--work-tree", "--namespace"];
const BRANCHES: &[&str] = &["refs/heads"];
const TRACKING: &[&str] = &["refs/heads", "refs/remotes"];
const TAGS: &[&str] = &["refs/tags"];
const REVISIONS: &[&str] = &["refs/heads", "refs/remotes", "refs/tags"];

pub(super) fn config_words(words: &[String]) -> Option<&[String]> {
    let (_, rest) = global(words, GLOBAL, &[])?;
    rest.first().filter(|word| *word == "config").map(|_| &rest[1..])
}

fn remotes(scope: &[String], prefix: &str) -> Rows {
    query("git", scope, &["remote"]).lines().map(|name| {
        (format!("{prefix}{name}"), "Git remote".into())
    }).collect()
}

fn refs(scope: &[String], prefixes: &[&str], current: &str, revisions: bool) -> Rows {
    let mut command = vec![
        "for-each-ref",
        "--format=%(refname:short)%09%(objectname:short)%09%(upstream:short)%09%(subject)",
        "--",
    ];
    command.extend_from_slice(prefixes);
    let (prefix, tail) = if revisions {
        current.rsplit_once("...").map(|(lhs, rhs)| (&current[..lhs.len() + 3], rhs))
            .or_else(|| current.rsplit_once("..").map(|(lhs, rhs)| (&current[..lhs.len() + 2], rhs)))
            .unwrap_or_else(|| current.strip_prefix('^').map_or(("", current), |value| ("^", value)))
    } else {
        ("", current)
    };
    let data = query("git", scope, &command);
    let mut rows: Rows = data.lines().filter_map(|line| {
        let (name, desc) = line.split_once('\t')?;
        name.starts_with(tail).then(|| (
            format!("{prefix}{name}"),
            metadata(&desc.split('\t').map(str::to_owned).collect::<Vec<_>>()),
        ))
    }).collect();
    if revisions && !data.is_empty() && "HEAD".starts_with(tail) {
        rows.push((format!("{prefix}HEAD"), "Current commit".into()));
    }
    rows
}

fn paths(scope: &[String], mode: &str) -> Rows {
    let (args, label) = match mode {
        "changed" => (&["ls-files", "-z", "--modified", "--deleted", "--others", "--exclude-standard", "--"][..], "Changed or untracked path"),
        "conflict" => (&["ls-files", "-z", "--unmerged", "--"][..], "Unmerged path"),
        _ => (&["ls-files", "-z", "--cached", "--"][..], "Tracked path"),
    };
    query("git", scope, args).split_terminator('\0').filter_map(|word| {
        let path = if mode == "conflict" { word.split_once('\t')?.1 } else { word };
        Some((path.to_owned(), label.into()))
    }).collect()
}

fn tree_paths(scope: &[String], current: &str) -> Option<Rows> {
    let (revision, _) = current.split_once(':')?;
    if revision.is_empty() || revision.starts_with('-') ||
        !revision.bytes().all(|b| b.is_ascii_alphanumeric() || b"/_.-~^@{}".contains(&b))
    {
        return Some(vec![]);
    }
    let output = query("git", scope, &["ls-tree", "-rz", "--name-only", revision, "--"]);
    Some(output.split_terminator('\0').map(|path| {
        (format!("{revision}:{path}"), "Path in selected Git tree".into())
    }).collect())
}

fn worktrees(scope: &[String]) -> Rows {
    query("git", scope, &["worktree", "list", "--porcelain", "-z"])
        .split("\0\0").filter_map(|record| {
            let mut fields = record.split('\0');
            let path = fields.next()?.strip_prefix("worktree ")?;
            let mut label = "Git worktree";
            for field in fields {
                if let Some(branch) = field.strip_prefix("branch ") {
                    label = branch.strip_prefix("refs/heads/").unwrap_or(branch);
                } else if field == "detached" {
                    label = "Detached HEAD";
                }
            }
            Some((path.into(), label.into()))
        }).collect()
}

fn stash(scope: &[String]) -> Rows {
    query("git", scope, &["--no-pager", "stash", "list", "--format=%gd%x09%gs"])
        .lines().filter_map(|row| row.split_once('\t'))
        .map(|(name, desc)| (name.into(), desc.into())).collect()
}

fn remote_refs(scope: &[String], remote: &str, current: &str) -> Rows {
    if !crate::probe::safe_name(remote) {
        return vec![];
    }
    let namespace = format!("refs/remotes/{remote}");
    let prefix = format!("{remote}/");
    refs(scope, &[&namespace], &format!("{prefix}{current}"), false).into_iter().filter_map(|(name, desc)| {
        let branch = name.strip_prefix(&prefix)?;
        (branch != "HEAD").then(|| (branch.into(), desc))
    }).collect()
}

fn push_refs(scope: &[String], remote: &str, delete: bool, current: &str) -> Rows {
    if let Some((source, destination)) = current.split_once(':') {
        return remote_refs(scope, remote, destination).into_iter()
            .map(|(name, desc)| (format!("{source}:{name}"), desc)).collect();
    }
    if delete {
        remote_refs(scope, remote, current)
    } else {
        refs(scope, &["refs/heads", "refs/tags"], current, false)
    }
}

fn submodules(scope: &[String]) -> Rows {
    query("git", scope, &["config", "--no-includes", "--file=.gitmodules", "--null", "--get-regexp", r"^submodule\..*\.path$"])
        .split_terminator('\0').filter_map(|row| row.split_once('\n'))
        .map(|(_, path)| (path.into(), "Configured submodule".into())).collect()
}

pub(super) fn candidates(words: &[String], current: &str) -> Rows {
    let Some((scope, words)) = global(words, GLOBAL, FORWARD) else { return vec![] };
    let Some((action, rest)) = words.split_first() else {
        if current.is_empty() || current.starts_with('-') { return vec![] }
        return query("git", &scope, &["--list-cmds=main,others,nohelpers"])
            .lines().filter(|name| crate::probe::safe_name(name))
            .map(|name| (name.into(), "Git command".into())).collect();
    };
    if action == "config" { return super::git_config::candidates(&scope, rest, current) }
    let takes = |flag: &str| matches!(flag, "--create" | "--force-create" | "--orphan" | "--source"
        | "--format" | "--sort" | "--pathspec-from-file" | "--onto" | "--upstream" | "--message"
        | "--remote" | "--default" | "--strategy" | "--strategy-option" | "--exec" | "--mainline"
        | "--ref" | "--base" | "--head" | "--max-count" | "--author" | "--grep" | "--exclude"
        | "--depth" | "--origin" | "--upload-pack" | "--receive-pack")
        || match flag {
            "-b" | "-B" => matches!(action.as_str(), "checkout" | "worktree" | "clone"),
            "-c" | "-C" => matches!(action.as_str(), "switch" | "commit" | "tag" | "notes"),
            "-m" | "-F" => matches!(action.as_str(), "commit" | "tag" | "stash" | "notes" | "merge"),
            "-n" => matches!(action.as_str(), "log" | "show" | "reflog"),
            _ => false,
        };
    let args = arguments(rest, takes);
    let pos = args.positional.as_slice();
    let slot = args.pending.map(|flag| (flag, 0)).or_else(|| {
        value_option(current, takes).filter(|(_, offset)| *offset < current.len() || current.ends_with('='))
    });
    if let Some((flag, offset)) = slot {
        let tail = &current[offset..];
        let rows = match flag {
            "--source" | "--onto" | "--upstream" | "--base" | "--head" => refs(&scope, REVISIONS, tail, true),
            "--ref" if action == "notes" => refs(&scope, &["refs/notes"], tail, false),
            _ => return vec![],
        };
        return rows.into_iter().map(|(name, desc)| (format!("{}{name}", &current[..offset]), desc)).collect();
    }
    if args.options && current.starts_with('-') { return vec![] }
    if !args.options {
        return match action.as_str() {
            "add" => paths(&scope, "changed"),
            "checkout" | "restore" | "reset" | "diff" | "log" | "show" | "rm" | "mv" | "grep" | "blame" | "ls-files" | "commit" | "status" | "clean" => paths(&scope, "tracked"),
            "submodule" => submodules(&scope),
            _ => vec![],
        };
    }
    if matches!(action.as_str(), "show" | "cat-file") {
        if let Some(rows) = tree_paths(&scope, current) { return rows }
    }
    // Positions are resolved after consuming option values. Ref lists and
    // path inventories remain independent so a later operand stays editable.
    match (action.as_str(), pos) {
        ("remote", ["show" | "get-url" | "set-url" | "remove" | "rm" | "rename" | "prune" | "update" | "set-head" | "set-branches"]) |
        ("push" | "fetch" | "pull" | "ls-remote", []) => remotes(&scope, ""),
        ("remote", ["set-head" | "set-branches", remote, ..]) | ("fetch" | "pull", [remote, ..]) =>
            remote_refs(&scope, remote, current),
        ("worktree", ["remove" | "move" | "lock" | "unlock"]) => worktrees(&scope),
        ("worktree", ["add", _]) => refs(&scope, REVISIONS, current, true),
        ("stash", ["apply" | "pop" | "drop" | "show"]) | ("stash", ["branch", _]) => stash(&scope),
        ("submodule", ["init" | "deinit" | "update" | "status" | "sync" | "summary", ..]) => submodules(&scope),
        ("add", _) if args.switches.contains(&"--refresh") => paths(&scope, "tracked"),
        ("add", _) => paths(&scope, "changed"),
        ("restore", _) if args.switches.contains(&"--merge") => paths(&scope, "conflict"),
        ("restore" | "rm" | "mv" | "blame" | "ls-files", _) => paths(&scope, "tracked"),
        ("push", [remote, ..]) => push_refs(&scope, remote, args.switches.iter().any(|flag| matches!(*flag, "--delete" | "-d")), current),
        ("switch", []) => refs(&scope, TRACKING, current, false),
        ("branch", _) if args.switches.iter().any(|f| matches!(*f, "-d"|"-D"|"-m"|"-M"|"-c"|"-C"|"--delete"|"--move"|"--copy"|"--edit-description")) => refs(&scope, BRANCHES, current, false),
        ("tag", _) if args.switches.iter().any(|f| matches!(*f, "-d"|"-v"|"--delete"|"--verify")) => refs(&scope, TAGS, current, false),
        ("notes", ["show" | "edit" | "remove" | "append" | "add" | "copy", ..]) => refs(&scope, REVISIONS, current, true),
        ("bisect", ["good" | "bad" | "old" | "new" | "start" | "skip" | "reset", ..]) => refs(&scope, REVISIONS, current, true),
        ("log" | "show" | "diff" | "rev-list" | "merge" | "rebase" | "cherry-pick" | "revert" | "reset" | "merge-base" | "describe" | "archive" | "cat-file" | "rev-parse", _) |
        ("checkout", []) | ("tag", [_]) => refs(&scope, REVISIONS, current, true),
        _ => vec![],
    }
}
