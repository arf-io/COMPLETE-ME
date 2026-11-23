//! ZFS inventory and local paths relative to a mounted dataset.
use super::{arguments, proxmox::Endpoint, Rows};
use std::path::{Component, Path, PathBuf};

const ZFS_ACTIONS: &[&str] = &[
    "clone", "create", "destroy", "get", "hold", "holds", "list", "mount", "promote", "receive",
    "release", "rename", "rollback", "send", "set", "snapshot", "unmount",
];
const ZPOOL_ACTIONS: &[&str] = &[
    "clear", "destroy", "export", "get", "history", "iostat", "list", "offline", "online",
    "remove", "replace", "scrub", "set", "status", "trim", "upgrade",
];

fn dataset_name(name: &str) -> bool {
    name.split('/').all(|part| {
        !part.is_empty()
            && !matches!(part, "." | "..")
            && part.as_bytes()[0] != b'-'
            && part
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-.:%".contains(&b))
    })
}

fn datasets(node: Endpoint<'_>, kind: &str) -> Rows {
    node.query(
        "zfs",
        &["list", "-H", "-t", kind, "-o", "name,type,mountpoint"],
        "ZFS dataset",
    )
    .lines()
    .filter_map(|line| {
        let mut fields = line.split('\t');
        let name = fields.next()?;
        let kind = fields.next()?;
        let mount = fields.next()?;
        let desc = if kind == "filesystem" && mount.starts_with('/') {
            format!("{kind} · {mount}")
        } else {
            kind.into()
        };
        Some((name.into(), desc))
    })
    .collect()
}

pub(super) fn candidates(node: Endpoint<'_>, tool: &str, words: &[String], current: &str) -> Rows {
    let actions = if tool == "zfs" {
        ZFS_ACTIONS
    } else {
        ZPOOL_ACTIONS
    };
    let Some((action, rest)) = words.split_first() else {
        return actions
            .iter()
            .map(|name| ((*name).into(), format!("{tool} command")))
            .collect();
    };
    let action = action.as_str();
    if !actions.contains(&action) || current.starts_with('-') {
        return vec![];
    }
    let args = arguments(rest, |flag| {
        matches!(flag, "-o" | "-O" | "-t" | "-s" | "-S" | "-d" | "-c")
            || (tool == "zfs" && action == "send" && matches!(flag, "-i" | "-I"))
    });
    if let Some(flag) = args.pending {
        return if tool == "zfs" && action == "send" && matches!(flag, "-i" | "-I") {
            datasets(node, "snapshot,bookmark")
        } else {
            vec![]
        };
    }
    let pos = args.positional.as_slice();
    if tool == "zpool" {
        if (matches!(action, "get" | "set") && pos.len() == 1)
            || (!matches!(action, "get" | "set") && pos.is_empty())
            || matches!(action, "list" | "status" | "iostat")
        {
            return node
                .query("zpool", &["list", "-H", "-o", "name,health"], "ZFS pool")
                .lines()
                .filter_map(|line| {
                    let (name, health) = line.split_once('\t')?;
                    Some((name.into(), health.into()))
                })
                .collect();
        }
        return vec![];
    }

    let selected_type = args
        .values
        .iter()
        .rev()
        .find_map(|&(flag, kind)| (flag == "-t").then_some(kind));
    let kind = match (action, pos) {
        ("list", [..]) | ("get", [_, ..]) if selected_type.is_some() => {
            let selected = selected_type.unwrap();
            if !selected.split(',').all(|kind| {
                matches!(
                    kind,
                    "filesystem" | "volume" | "snapshot" | "bookmark" | "all"
                )
            }) {
                return vec![];
            }
            selected
        }
        ("rollback" | "clone", []) | ("hold" | "release", [_]) | ("holds", [..]) => "snapshot",
        ("send", []) if current.contains('#') => "bookmark",
        ("send", []) => "filesystem,volume,snapshot",
        ("get" | "set", [_, ..]) | ("list", [..]) | ("destroy" | "rename", [])
            if current.contains('@') =>
        {
            "snapshot"
        }
        ("snapshot", [..]) if !current.contains('@') => "filesystem,volume",
        ("get" | "set", [_, ..])
        | ("list", [..])
        | ("destroy" | "rename" | "promote" | "receive", []) => "filesystem,volume",
        ("mount" | "unmount", []) => "filesystem",
        _ => return vec![],
    };
    datasets(node, kind)
}

fn legacy_mountpoint(dataset: &str) -> Option<PathBuf> {
    let node = Endpoint::local();
    if cfg!(target_os = "linux") {
        let output = node.query(
            "findmnt",
            &[
                "--json", "--types", "zfs", "--source", dataset, "--output", "TARGET",
            ],
            "legacy ZFS mount",
        );
        let output: serde_json::Value = serde_json::from_str(&output).ok()?;
        let [mount] = output["filesystems"].as_array()?.as_slice() else {
            return None;
        };
        return mount["target"].as_str().map(PathBuf::from);
    }
    let mounted = node.query("mount", &[], "legacy ZFS mount");
    let prefix = format!("{dataset} on ");
    let mut paths = mounted.lines().filter_map(|line| {
        let (path, details) = line.strip_prefix(&prefix)?.rsplit_once(" (")?;
        (details.starts_with("zfs,") || details == "zfs)").then(|| PathBuf::from(path))
    });
    let path = paths.next()?;
    paths.next().is_none().then_some(path)
}

fn mounted_root(dataset: &str) -> Option<PathBuf> {
    if !dataset_name(dataset) {
        return None;
    }
    let out = Endpoint::local().query(
        "zfs",
        &[
            "list",
            "-H",
            "-t",
            "filesystem",
            "-o",
            "mountpoint,mounted",
            dataset,
        ],
        "mounted ZFS path",
    );
    let (path, mounted) = out.trim_end_matches(['\n', '\r']).split_once('\t')?;
    let path = if path == "legacy" {
        legacy_mountpoint(dataset)?
    } else if mounted == "yes" {
        PathBuf::from(path)
    } else {
        return None;
    };
    if !path.is_absolute() {
        return None;
    }
    std::fs::canonicalize(path)
        .ok()
        .filter(|path| path.is_dir())
}

fn contained_path(root: &Path, relative: &str) -> Option<PathBuf> {
    let relative = Path::new(relative);
    if relative
        .components()
        .any(|component| !matches!(component, Component::Normal(_) | Component::CurDir))
    {
        return None;
    }
    let path = root.join(relative).canonicalize().ok()?;
    path.starts_with(root).then_some(path)
}

pub(super) fn path_candidates(words: &[String], current: &str) -> Rows {
    match words {
        [] if !current.starts_with('-') => datasets(Endpoint::local(), "filesystem"),
        [dataset] => {
            let Some(root) = mounted_root(dataset) else {
                return vec![];
            };
            let (prefix, leaf) = current
                .rsplit_once('/')
                .map_or(("", current), |(parent, leaf)| {
                    (&current[..parent.len() + 1], leaf)
                });
            let Some(dir) = contained_path(&root, prefix) else {
                return vec![];
            };
            let Ok(entries) = std::fs::read_dir(dir) else {
                return vec![];
            };
            let mut rows: Rows = entries
                .filter_map(Result::ok)
                .filter_map(|entry| {
                    let name = entry.file_name().into_string().ok()?;
                    if !name.starts_with(leaf) || (name.starts_with('.') && !leaf.starts_with('.'))
                    {
                        return None;
                    }
                    let mut value = format!("{prefix}{name}");
                    let path = contained_path(&root, &value)?;
                    if path.is_dir() {
                        value.push('/');
                    }
                    Some((value, format!("Path in {dataset}")))
                })
                .collect();
            rows.sort_unstable();
            rows
        }
        _ => vec![],
    }
}

pub(crate) fn run_path(args: &[String]) -> i32 {
    let (dataset, relative) = match args {
        [dataset] => (dataset, ""),
        [dataset, relative] => (dataset, relative.as_str()),
        _ => {
            eprintln!("usage: complete-me zfs-path DATASET [RELATIVE-PATH]");
            return 2;
        }
    };
    let Some(root) = mounted_root(dataset) else {
        eprintln!("complete-me: expected a mounted local ZFS filesystem");
        return 1;
    };
    let Some(path) = contained_path(&root, relative) else {
        eprintln!("complete-me: path must exist inside the dataset");
        return 1;
    };
    println!("{}", path.display());
    0
}
