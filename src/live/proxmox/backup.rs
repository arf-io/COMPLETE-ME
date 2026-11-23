//! Read-only discovery for native Proxmox backup and restore commands.
use super::{host_name, metadata, vmid, Endpoint, Rows};
use crate::live::{arguments, field, value_option};
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Component, Path};

#[derive(Clone, Copy, Eq, PartialEq)]
enum GuestKind {
    Vm,
    Container,
}

impl GuestKind {
    fn resource(self) -> &'static str {
        match self {
            Self::Vm => "qemu",
            Self::Container => "lxc",
        }
    }

    fn storage_content(self) -> &'static str {
        match self {
            Self::Vm => "images",
            Self::Container => "rootdir",
        }
    }
}

fn storage_config(node: Endpoint<'_>, content: &str) -> Vec<Value> {
    // pvesm status calls backend activation. Read configuration instead.
    let output = node.query(
        "pvesh",
        &["get", "/storage", "--output-format", "json"],
        "storage configuration",
    );
    let rows: Vec<Value> = serde_json::from_str(&output).unwrap_or_default();
    let mut hostname = None;
    rows.into_iter()
        .filter(|row| {
            if !row["storage"].as_str().is_some_and(host_name)
                || !row["type"].as_str().is_some_and(host_name)
                || !row["content"]
                    .as_str()
                    .is_some_and(|all| all.split(',').any(|kind| kind == content))
                || matches!(row["disable"], Value::Bool(true))
                || row["disable"]
                    .as_u64()
                    .is_some_and(|disabled| disabled != 0)
            {
                return false;
            }
            row["nodes"].as_str().is_none_or(|nodes| {
                let hostname =
                    hostname.get_or_insert_with(|| node.query("hostname", &["-s"], "storage node"));
                host_name(hostname.trim()) && nodes.split(',').any(|name| name == hostname.trim())
            })
        })
        .collect()
}

fn browsable(backend: &str) -> bool {
    matches!(
        backend,
        "pbs" | "dir" | "nfs" | "cifs" | "cephfs" | "glusterfs"
    )
}

fn stores(node: Endpoint<'_>, content: &str, prefix: &str, suffix: &str) -> Rows {
    storage_config(node, content)
        .into_iter()
        .filter_map(|row| {
            let name = row["storage"].as_str()?;
            let backend = row["type"].as_str()?;
            (suffix.is_empty() || browsable(backend)).then(|| {
                (
                    format!("{prefix}{name}{suffix}"),
                    format!("{content} storage · {backend}"),
                )
            })
        })
        .collect()
}

fn directory_archives(
    node: Endpoint<'_>,
    row: &Value,
    store: &str,
    backend: &str,
) -> Vec<(String, u64)> {
    let Some(path) = row["path"].as_str().filter(|path| clean_path(path, true)) else {
        return vec![];
    };
    let mount = match (backend, row["is_mountpoint"].as_str()) {
        ("dir", None) if row["is_mountpoint"] == true || row["is_mountpoint"] == 1 => path,
        ("dir", None | Some("" | "0" | "no" | "off" | "false")) => "",
        ("dir", Some("1" | "yes" | "on" | "true")) => path,
        ("dir", Some(mount)) => mount,
        _ => path,
    };
    if !mount.is_empty() && !clean_path(mount, true) {
        return vec![];
    }
    let directory = row["content-dirs"]
        .as_str()
        .unwrap_or_default()
        .split(',')
        .find_map(|part| part.strip_prefix("backup="))
        .unwrap_or("dump");
    if !clean_path(directory, false) {
        return vec![];
    }
    let offline = if super::remote_disabled() { "1" } else { "0" };
    node.query(
        "perl",
        &[
            "-e",
            include_str!("archives.pl"),
            "--",
            path,
            directory,
            mount,
            backend,
            offline,
        ],
        "backup directory",
    )
    .split('\0')
    .filter_map(|entry| {
        let (mtime, name) = entry.split_once(' ')?;
        let time = mtime.split('.').next()?.parse().ok()?;
        Some((format!("{store}:backup/{name}"), time))
    })
    .collect()
}

fn clean_path(path: &str, absolute: bool) -> bool {
    !path.is_empty()
        && Path::new(path).is_absolute() == absolute
        && !path.chars().any(char::is_control)
        && Path::new(path)
            .components()
            .all(|c| matches!(c, Component::RootDir | Component::Normal(_)))
}

fn archive_identity(path: &str) -> Option<(GuestKind, &str, &str)> {
    if !path
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b"/._-:".contains(&b))
        || path.split('/').any(|part| matches!(part, "" | "." | ".."))
    {
        return None;
    }
    let path = path.strip_prefix("backup/")?;
    let (kind, id, stamp) = if let Some(rest) = path.strip_prefix("vzdump-") {
        let (kind, rest) = rest.split_once('-')?;
        let (id, dated) = rest.split_once('-')?;
        let (stamp, extension) = dated.split_once('.')?;
        let format = match extension.split_once('.') {
            Some((format, "gz" | "lzo" | "zst" | "bz2")) => format,
            Some(_) => return None,
            None => extension,
        };
        match kind {
            "qemu" if format == "vma" => (GuestKind::Vm, id, stamp),
            "lxc" | "openvz" if format == "tar" => (GuestKind::Container, id, stamp),
            _ => return None,
        }
    } else {
        let (kind, rest) = path.split_once('/')?;
        let (id, stamp) = rest.split_once('/')?;
        let kind = match kind {
            "vm" => GuestKind::Vm,
            "ct" => GuestKind::Container,
            _ => return None,
        };
        (kind, id, stamp)
    };
    // Directory storage uses local time, while PBS volume IDs use UTC.
    if !vmid(id)
        || stamp.len() < 19
        || stamp.len() > 20
        || !stamp
            .bytes()
            .all(|b| b.is_ascii_digit() || b"-_:TZ".contains(&b))
    {
        return None;
    }
    Some((kind, id, stamp))
}

fn archives(node: Endpoint<'_>, kind: GuestKind, current: &str) -> Rows {
    let Some((store, _)) = current.split_once(':') else {
        return stores(node, "backup", "", ":");
    };
    if !host_name(store) {
        return vec![];
    }
    let Some(config) = storage_config(node, "backup")
        .into_iter()
        .find(|row| row["storage"] == store)
    else {
        return vec![];
    };
    let backend = config["type"].as_str().unwrap_or_default();
    if super::remote_disabled() && backend != "dir" {
        crate::live::record(|| {
            serde_json::json!({
                "source":"proxmox", "slot":"backup archive", "status":"remote-disabled"
            })
        });
        return vec![];
    }
    let rows = match backend {
        "pbs" => {
            // The built-in PBS backend checks the datastore through the PBS API.
            let path = format!("/nodes/localhost/storage/{store}/content");
            let output = node.query(
                "pvesh",
                &[
                    "get",
                    &path,
                    "--content",
                    "backup",
                    "--output-format",
                    "json",
                ],
                "backup archive",
            );
            let rows: Vec<Value> = serde_json::from_str(&output).unwrap_or_default();
            rows.into_iter()
                .filter_map(|row| {
                    Some((
                        row["volid"].as_str()?.to_owned(),
                        row["ctime"].as_u64().unwrap_or_default(),
                    ))
                })
                .collect()
        }
        "dir" | "nfs" | "cifs" | "cephfs" | "glusterfs" => {
            directory_archives(node, &config, store, backend)
        }
        _ => vec![],
    };
    let mut archives: Vec<_> = rows
        .iter()
        .filter_map(|(volume, time)| {
            let (storage, path) = volume.split_once(':')?;
            if storage != store || !volume.starts_with(current) {
                return None;
            }
            let (actual, id, stamp) = archive_identity(path)?;
            if actual != kind {
                return None;
            }
            Some((volume, id, stamp, *time))
        })
        .collect();
    if archives.is_empty() {
        return vec![];
    }
    let output = node.query(
        "pvesh",
        &[
            "get",
            "/cluster/resources",
            "--type",
            "vm",
            "--output-format",
            "json",
        ],
        "backup guest names",
    );
    let guests: Vec<Value> = serde_json::from_str(&output).unwrap_or_default();
    let names: HashMap<_, _> = guests
        .iter()
        .filter(|guest| guest["type"] == kind.resource())
        .map(|guest| (field(guest, "vmid"), field(guest, "name")))
        .collect();
    archives.sort_unstable_by(|a, b| b.3.cmp(&a.3).then_with(|| b.2.cmp(a.2)));
    archives
        .into_iter()
        .map(|(volume, id, stamp, _)| {
            let description = metadata(&[
                format!("Guest {id}"),
                names
                    .get(id)
                    .filter(|name| !name.is_empty())
                    .map(|name| format!("current guest name: {name}"))
                    .unwrap_or_default(),
                stamp.into(),
                store.into(),
            ]);
            (volume.to_owned(), description)
        })
        .collect()
}

fn next_id(node: Endpoint<'_>, current: &str) -> Rows {
    if !current.bytes().all(|b| b.is_ascii_digit()) {
        return vec![];
    }
    let output = node.query(
        "pvesh",
        &["get", "/cluster/nextid", "--output-format", "json"],
        "unused guest ID",
    );
    let id = output.trim().trim_matches('"');
    if vmid(id) {
        vec![(id.into(), "Next unused guest ID at lookup time".into())]
    } else {
        vec![]
    }
}

pub(super) fn is_command(tool: &str, words: &[String]) -> bool {
    matches!(tool, "vzdump" | "qmrestore")
        || tool == "pct" && words.first().is_some_and(|word| word == "restore")
}

pub(super) fn candidates(node: Endpoint<'_>, tool: &str, words: &[String], current: &str) -> Rows {
    let takes_value = |flag: &str| {
        matches!(
            flag,
            "--storage"
                | "--mode"
                | "--compress"
                | "--node"
                | "--dumpdir"
                | "--exclude"
                | "--notes-template"
                | "--bwlimit"
                | "--pool"
                | "--hostname"
                | "--rootfs"
                | "--arch"
                | "--ostype"
                | "--password"
                | "--ssh-public-keys"
                | "--unique"
                | "--force"
                | "--live-restore"
                | "--start"
                | "--unprivileged"
        )
    };
    let args = arguments(words, takes_value);
    if !args.options
        || args.values.iter().any(|(flag, _)| *flag == "--node")
        || args.switches.iter().any(|flag| *flag != "--all")
    {
        return vec![];
    }
    let option = args.pending.map(|flag| (flag, 0)).or_else(|| {
        value_option(current, takes_value)
            .filter(|(_, offset)| *offset < current.len() || current.ends_with('='))
    });
    let kind = if tool == "pct" {
        GuestKind::Container
    } else {
        GuestKind::Vm
    };
    if let Some((flag, offset)) = option {
        return if flag == "--storage" && !args.values.iter().any(|(name, _)| *name == "--dumpdir") {
            let content = if tool == "vzdump" {
                "backup"
            } else {
                kind.storage_content()
            };
            stores(node, content, &current[..offset], "")
        } else {
            vec![]
        };
    }
    if current.starts_with('-') {
        return vec![("--storage=".into(), "Select storage on this node".into())];
    }
    match (tool, args.positional.as_slice()) {
        ("qmrestore", []) => archives(node, kind, current),
        ("pct", [id]) if vmid(id) => archives(node, kind, current),
        ("qmrestore", [_]) | ("pct", []) => next_id(node, current),
        ("vzdump", _) if !args.switches.contains(&"--all") => {
            let rows: Vec<Value> = serde_json::from_str(&node.query(
                "pvesh",
                &[
                    "get",
                    "/cluster/resources",
                    "--type",
                    "vm",
                    "--output-format",
                    "json",
                ],
                "backup guest ID",
            ))
            .unwrap_or_default();
            // vzdump targets this execution node. Resolve its actual name rather than using the SSH alias.
            let hostname = node.query("hostname", &["-s"], "backup node");
            let hostname = hostname.trim();
            if !host_name(hostname) {
                return vec![];
            }
            rows.iter()
                .filter_map(|row| {
                    let id = field(row, "vmid");
                    let kind = row["type"].as_str()?;
                    if row["node"] != hostname
                        || !matches!(kind, "qemu" | "lxc")
                        || !vmid(&id)
                        || args.positional.contains(&id.as_str())
                    {
                        return None;
                    }
                    Some((
                        id,
                        metadata(&[field(row, "name"), kind.into(), hostname.into()]),
                    ))
                })
                .collect()
        }
        _ => vec![],
    }
}
