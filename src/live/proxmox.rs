//! Local Proxmox inventories and explicitly configured SSH destinations.
use super::{arguments, metadata, remote_disabled, Rows};
use crate::util::{config_home, open_regular, query_result, QueryStatus};
use serde_json::{json, Value};
use std::io::Read;
use std::net::IpAddr;
use std::os::unix::process::CommandExt;
use std::process::Command;
use std::time::{Duration, Instant};

mod backup;
mod cluster;

const TOOLS: &[&str] = &["pct", "qm", "pvesm", "vzdump", "qmrestore", "zfs", "zpool"];

fn host_name(word: &str) -> bool {
    !word.is_empty()
        && word.as_bytes()[0].is_ascii_alphanumeric()
        && word
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}

fn hosts() -> Vec<String> {
    let path = config_home().join("complete-me/proxmox-hosts");
    let Ok(file) = open_regular(path) else {
        return vec![];
    };
    let mut text = String::new();
    if file.take(65_537).read_to_string(&mut text).is_err() || text.len() > 65_536 {
        return vec![];
    }
    text.lines()
        .map(|line| line.split('#').next().unwrap_or_default().trim())
        .filter(|alias| host_name(alias))
        .map(str::to_owned)
        .collect()
}

fn remote_command(tool: &str, args: &[&str]) -> String {
    // The login shell parses SSH's command string. Quote for both POSIX shells and fish.
    fn word(value: &str) -> String {
        let mut quoted = String::from("'");
        for ch in value.chars() {
            match ch {
                '\'' => quoted.push_str("'\"'\"'"),
                '\\' => quoted.push_str("'\"\\\\\"'"),
                _ => quoted.push(ch),
            }
        }
        quoted.push('\'');
        quoted
    }
    std::iter::once(tool)
        .chain(args.iter().copied())
        .map(word)
        .collect::<Vec<_>>()
        .join(" ")
}

fn checked_command(node: &str, args: impl IntoIterator<Item = impl AsRef<str>>) -> String {
    const CHECK: &str = r#"if [ "$(hostname -s)" != "$1" ]; then echo "complete-me: SSH host does not match selected Proxmox node" >&2; exit 125; fi; shift; if [ "$#" -eq 0 ]; then exec "${SHELL:-/bin/sh}" -l; fi; exec "$@""#;
    let args: Vec<_> = args.into_iter().collect();
    let command: Vec<_> = ["-c", CHECK, "complete-me", node]
        .into_iter()
        .chain(args.iter().map(AsRef::as_ref))
        .collect();
    remote_command("sh", &command)
}

#[cfg(test)]
mod tests {
    use super::remote_command;
    use std::process::Command;

    #[test]
    fn remote_arguments_round_trip_through_login_shells() {
        let value = r"West's backup\2026\";
        let command = remote_command("printf", &["%s", value]);
        for shell in ["sh", "bash", "zsh", "fish"] {
            match Command::new(shell).args(["-c", &command]).output() {
                Ok(output) => {
                    assert!(output.status.success(), "{shell}");
                    assert_eq!(output.stdout, value.as_bytes(), "{shell}");
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => panic!("{shell}: {e}"),
            }
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct Endpoint<'a> {
    host: Option<&'a str>,
    node: Option<&'a str>,
}

impl<'a> Endpoint<'a> {
    pub(super) fn local() -> Self {
        Self {
            host: None,
            node: None,
        }
    }

    fn ssh(host: &'a str) -> Self {
        Self {
            host: Some(host),
            node: None,
        }
    }

    pub(super) fn query(self, tool: &str, args: &[&str], slot: &str) -> String {
        self.request(tool, args, slot, true)
    }

    fn request(self, tool: &str, args: &[&str], slot: &str, completing: bool) -> String {
        let mut argv;
        let deadline = if let Some(host) = self.host {
            if completing && remote_disabled() {
                super::record(
                    || json!({"source":"proxmox", "host":host, "slot":slot, "status":"remote-disabled"}),
                );
                return String::new();
            }
            argv = [
                "ssh",
                "-T",
                "-o",
                "BatchMode=yes",
                "-o",
                "StrictHostKeyChecking=yes",
                "-o",
                "ConnectTimeout=1",
                "-o",
                "ClearAllForwardings=yes",
                "-o",
                "ForwardAgent=no",
                "-o",
                "ForwardX11=no",
                "-o",
                "PermitLocalCommand=no",
                "--",
                host,
            ]
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>();
            argv.push(match self.node {
                Some(node) => {
                    checked_command(node, std::iter::once(tool).chain(args.iter().copied()))
                }
                None => remote_command(tool, args),
            });
            Duration::from_millis(1400)
        } else {
            argv = std::iter::once(tool)
                .chain(args.iter().copied())
                .map(str::to_owned)
                .collect();
            Duration::from_millis(350)
        };
        let start = Instant::now();
        let (output, status) = query_result(&argv, deadline);
        super::record(|| {
            json!({
                "source": tool, "operation": args.first(), "host": self.host, "slot": slot,
                "elapsed_ms": start.elapsed().as_millis() as u64,
                "status": match status {
                    QueryStatus::Success if output.trim().is_empty() => "empty",
                    QueryStatus::Success => "success",
                    QueryStatus::Timeout => "timeout",
                    QueryStatus::Missing => "tool-missing",
                    QueryStatus::Failed(_) => "failed",
                }
            })
        });
        output
    }
}

pub(crate) fn run(args: &[String]) -> i32 {
    let [host, rest @ ..] = args else {
        eprintln!("usage: complete-me proxmox HOST [pct|qm|pvesm|vzdump|qmrestore|zfs|zpool ARG…]");
        return 2;
    };
    let hosts = hosts();
    if let Some(cluster) = cluster::configured(&hosts).get(host) {
        return cluster.run(rest);
    }
    if !hosts.contains(host) {
        eprintln!("complete-me: configure this SSH alias or cluster in complete-me/proxmox-hosts or proxmox-clusters");
        return 2;
    }
    node_command(host, None, rest)
}

fn node_command(host: &str, node: Option<&str>, rest: &[String]) -> i32 {
    let mut ssh = Command::new("ssh");
    let tty = match rest {
        [] => true,
        [tool, args @ ..] if TOOLS.contains(&tool.as_str()) => interactive(tool, args),
        _ => {
            eprintln!("complete-me: expected a Proxmox node command");
            return 2;
        }
    };
    ssh.arg(if tty { "-t" } else { "-T" })
        .args(["-o", "StrictHostKeyChecking=yes", "--", host]);
    if let Some(node) = node {
        ssh.arg(checked_command(node, rest.iter()));
    } else if let Some((tool, args)) = rest.split_first() {
        ssh.arg(remote_command(
            tool,
            &args.iter().map(String::as_str).collect::<Vec<_>>(),
        ));
    }
    let error = ssh.exec();
    eprintln!("complete-me: could not start ssh: {error}");
    1
}

fn interactive(tool: &str, args: &[String]) -> bool {
    matches!(
        (tool, args.first().map(String::as_str)),
        ("pct", Some("enter" | "console")) | ("qm", Some("terminal" | "monitor"))
    )
}

pub(super) fn shortcut(words: &[String], current: &str) -> Rows {
    let hosts = hosts();
    let clusters = cluster::configured(&hosts);
    let Some((target, words)) = words.split_first() else {
        return hosts
            .into_iter()
            .map(|host| (host, "Proxmox SSH node".into()))
            .chain(
                clusters
                    .into_keys()
                    .map(|name| (name, "Proxmox cluster".into())),
            )
            .collect();
    };
    let cluster = clusters.get(target);
    if let (Some(cluster), [option, rest @ ..]) = (cluster, words) {
        if option == "--node" {
            let Some((name, command)) = rest.split_first() else {
                return cluster.node_candidates();
            };
            let Some(alias) = cluster.node_alias(name) else {
                return vec![];
            };
            return node_candidates(alias, name, command, current);
        }
    }
    let tools = if cluster.is_some() {
        &["pct", "qm", "--node"][..]
    } else if hosts.contains(target) {
        TOOLS
    } else {
        return vec![];
    };
    if let Some((tool, args)) = words.split_first() {
        if tools.contains(&tool.as_str()) {
            return candidates_at(Endpoint::ssh(target), cluster, tool, args, current);
        }
        return vec![];
    }
    tools
        .iter()
        .map(|name| ((*name).into(), "Proxmox command".into()))
        .collect()
}

fn node_candidates(alias: &str, name: &str, words: &[String], current: &str) -> Rows {
    if let Some((tool, args)) = words.split_first() {
        if TOOLS.contains(&tool.as_str()) {
            return candidates(
                Endpoint {
                    host: Some(alias),
                    node: Some(name),
                },
                tool,
                args,
                current,
            );
        }
        return vec![];
    }
    TOOLS
        .iter()
        .map(|tool| ((*tool).into(), "Proxmox node command".into()))
        .collect()
}

const GUEST_ACTIONS: &[&str] = &[
    "clone",
    "config",
    "console",
    "destroy",
    "migrate",
    "pending",
    "reboot",
    "resume",
    "rollback",
    "set",
    "shutdown",
    "snapshot",
    "start",
    "status",
    "stop",
    "suspend",
    "template",
    "unlock",
    "delsnapshot",
    "listsnapshot",
];
const CT_ACTIONS: &[&str] = &[
    "enter",
    "exec",
    "pull",
    "push",
    "df",
    "mount",
    "unmount",
    "move-volume",
    "move_volume",
    "remote-migrate",
    "resize",
    "fsck",
    "fstrim",
];
const VM_ACTIONS: &[&str] = &[
    "agent",
    "guest",
    "disk",
    "cloudinit",
    "importdisk",
    "importovf",
    "cleanup",
    "enroll-efi-keys",
    "terminal",
    "monitor",
    "reset",
    "wait",
    "showcmd",
    "move_disk",
    "remote-migrate",
    "move-disk",
    "resize",
];

fn vmid(text: &str) -> bool {
    text.len() <= 9
        && !text.starts_with('0')
        && text.bytes().all(|b| b.is_ascii_digit())
        && text.parse::<u32>().is_ok_and(|id| id >= 100)
}

fn address(raw: &str) -> Option<IpAddr> {
    let ip: IpAddr = raw.split('/').next()?.parse().ok()?;
    (!ip.is_loopback()
        && !ip.is_unspecified()
        && !ip.is_multicast()
        && match ip {
            IpAddr::V4(addr) => !addr.is_link_local(),
            IpAddr::V6(addr) => !addr.is_unicast_link_local(),
        })
    .then_some(ip)
}

fn guest_addresses(node: Endpoint<'_>, tool: &str, id: &str) -> Vec<String> {
    let mut ips: Vec<_> = if tool == "pct" {
        let config = node.query("pct", &["config", id], "configured container address");
        config
            .lines()
            .filter_map(|line| line.split_once(':'))
            .filter(|(key, _)| {
                key.strip_prefix("net")
                    .is_some_and(|v| !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit()))
            })
            .flat_map(|(_, value)| value.split(',').map(str::trim))
            .filter_map(|field| {
                field
                    .strip_prefix("ip=")
                    .or_else(|| field.strip_prefix("ip6="))
            })
            .filter_map(address)
            .collect()
    } else {
        let response = node.query(
            "qm",
            &["guest", "cmd", id, "network-get-interfaces"],
            "QEMU guest address",
        );
        let value: Value = serde_json::from_str(&response).unwrap_or_default();
        value
            .as_array()
            .or_else(|| value["result"].as_array())
            .into_iter()
            .flatten()
            .flat_map(|interface| interface["ip-addresses"].as_array().into_iter().flatten())
            .filter_map(|entry| entry["ip-address"].as_str().and_then(address))
            .collect()
    };
    ips.sort_unstable();
    ips.dedup();
    ips.into_iter().take(2).map(|ip| ip.to_string()).collect()
}

fn running_only(tool: &str, action: &str) -> bool {
    matches!(
        (tool, action),
        ("pct", "enter" | "exec" | "console") | ("qm", "terminal" | "monitor" | "guest" | "agent")
    )
}

fn guests(node: Endpoint<'_>, tool: &str, action: &str, prefix: &str) -> Rows {
    let running = running_only(tool, action);
    let result = node.query(
        tool,
        &["list"],
        if tool == "pct" {
            "container ID"
        } else {
            "VM ID"
        },
    );
    let ct_name_column = result
        .lines()
        .next()
        .filter(|header| tool == "pct" && header.starts_with("VMID") && header.contains("Lock"))
        .and_then(|header| header.find("Name"));
    let mut rows: Vec<_> = result
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let id = fields.next()?;
            if !vmid(id) || !id.starts_with(prefix) {
                return None;
            }
            let field = fields.next()?;
            let (name, state) = if tool == "pct" {
                // pct prints the name after a fixed-width, possibly empty lock column.
                let has_name = ct_name_column.is_none_or(|column| {
                    line.get(column..)
                        .is_some_and(|tail| !tail.trim().is_empty())
                });
                let name = if has_name {
                    fields.last().unwrap_or_default()
                } else {
                    ""
                };
                (name, field)
            } else {
                (field, fields.next()?)
            };
            if running && state != "running" {
                return None;
            }
            let desc = metadata(&[
                name.into(),
                state.into(),
                node.node.or(node.host).unwrap_or("local node").into(),
            ]);
            Some((id.to_owned(), desc, state == "running"))
        })
        .collect();
    if rows.len() == 1 && std::env::var("COMPLETE_ME_PROXMOX_IP").as_deref() == Ok("1") {
        let (id, desc, running) = &mut rows[0];
        if tool == "pct" || *running {
            let ips = guest_addresses(node, tool, id);
            if !ips.is_empty() {
                desc.push_str(" · ");
                desc.push_str(&ips.join(", "));
            }
        }
    }
    rows.into_iter().map(|(id, desc, _)| (id, desc)).collect()
}

fn snapshots(node: Endpoint<'_>, tool: &str, id: &str) -> Rows {
    node.query(tool, &["listsnapshot", id], "snapshot name")
        .lines()
        .filter_map(|line| {
            let name = line.split_once("->")?.1.split_whitespace().next()?;
            (name != "current" && host_name(name))
                .then(|| (name.into(), format!("Snapshot of {id}")))
        })
        .collect()
}

fn nodes(node: Endpoint<'_>) -> Rows {
    let response = node.query(
        "pvesh",
        &["get", "/nodes", "--output-format", "json"],
        "cluster node",
    );
    let nodes: Vec<Value> = serde_json::from_str(&response).unwrap_or_default();
    nodes
        .iter()
        .filter_map(|node| {
            let name = node["node"].as_str().filter(|name| host_name(name))?;
            Some((
                name.into(),
                node["status"].as_str().unwrap_or("Cluster node").into(),
            ))
        })
        .collect()
}

fn storage(node: Endpoint<'_>, suffix: &str) -> Rows {
    node.query("pvesm", &["status"], "storage ID")
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let name = fields.next()?;
            let kind = fields.next()?;
            let state = fields.next()?;
            (name != "Name" && host_name(name))
                .then(|| (format!("{name}{suffix}"), format!("{kind} · {state}")))
        })
        .collect()
}

fn volumes(node: Endpoint<'_>, current: &str) -> Rows {
    let Some((store, _)) = current.split_once(':') else {
        return storage(node, ":");
    };
    if !host_name(store) {
        return vec![];
    }
    let result = node.query("pvesm", &["list", store], "volume ID");
    result
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let id = fields.next()?;
            let prefix = id.split_once(':')?.0;
            (prefix == store).then(|| (id.into(), fields.take(3).collect::<Vec<_>>().join(" · ")))
        })
        .collect()
}

fn subcommands(tool: &str) -> &[&str] {
    if tool == "pct" {
        CT_ACTIONS
    } else {
        VM_ACTIONS
    }
}

fn nested(tool: &str, group: &str) -> &'static [&'static str] {
    match (tool, group) {
        ("qm", "guest") => &["cmd", "exec", "exec-status", "passwd"],
        ("qm", "disk") => &["import", "move", "resize", "unlink"],
        ("qm", "cloudinit") => &["dump", "pending", "update"],
        _ => &[],
    }
}

fn guest_action<'a>(
    tool: &str,
    action: &'a str,
    rest: &'a [String],
) -> Option<(&'a str, &'a [String])> {
    if !matches!(tool, "qm" | "pct") {
        return None;
    }
    let children = nested(tool, action);
    if children.is_empty() {
        return (GUEST_ACTIONS.contains(&action) || subcommands(tool).contains(&action))
            .then_some((action, rest));
    }
    let (sub, tail) = rest.split_first()?;
    if !children.contains(&sub.as_str()) {
        return None;
    }
    let slot = match (action, sub.as_str()) {
        ("disk", "import") => "importdisk",
        ("disk", "move") => "move-disk",
        _ => action,
    };
    Some((slot, tail))
}

pub(super) fn candidates(node: Endpoint<'_>, tool: &str, words: &[String], current: &str) -> Rows {
    candidates_at(node, None, tool, words, current)
}

fn candidates_at(
    node: Endpoint<'_>,
    cluster: Option<&cluster::Cluster>,
    tool: &str,
    words: &[String],
    current: &str,
) -> Rows {
    if matches!(tool, "zfs" | "zpool") {
        return super::zfs::candidates(node, tool, words, current);
    }
    if cluster.is_none() && backup::is_command(tool, words) {
        let args = if tool == "pct" { &words[1..] } else { words };
        return backup::candidates(node, tool, args, current);
    }
    let Some((action, rest)) = words.split_first() else {
        let actions: &[&str] = match tool {
            "pct" => CT_ACTIONS,
            "qm" => VM_ACTIONS,
            _ => &[
                "alloc",
                "free",
                "list",
                "path",
                "status",
                "prune-backups",
                "help",
            ],
        };
        let common: &[&str] = match (cluster.is_none(), tool) {
            (true, "pct") => &["list", "help", "restore"],
            (true, "qm") => &["list", "help"],
            _ => &[],
        };
        return common
            .iter()
            .chain(actions)
            .chain(GUEST_ACTIONS.iter().filter(|_| tool != "pvesm"))
            .map(|action| ((*action).into(), format!("{tool} command")))
            .collect();
    };
    // Unsupported leading switches leave native completion available. The
    // fixed queries below never forward command-line flags to Proxmox.
    if current.starts_with('-') {
        return vec![];
    }
    if tool == "pvesm" {
        let args = arguments(rest, |flag| {
            matches!(flag, "--content" | "--vmid" | "--format")
        });
        if args.pending.is_some() || !args.switches.is_empty() {
            return vec![];
        }
        return match (action.as_str(), args.positional.as_slice()) {
            ("alloc" | "list" | "prune-backups", []) => storage(node, ""),
            ("path" | "free", []) => volumes(node, current),
            _ => vec![],
        };
    }
    let children = nested(tool, action);
    if rest.is_empty() && !children.is_empty() {
        return children
            .iter()
            .map(|name| ((*name).into(), format!("qm {action} command")))
            .collect();
    }
    let Some((action, rest)) = guest_action(tool, action, rest) else {
        return vec![];
    };
    if let Some(cluster) = cluster {
        let Some(id) = rest.first() else {
            return cluster.candidates(tool, action, current);
        };
        // Routed commands take the ID directly after their action.
        if !vmid(id) {
            return vec![];
        }
    }
    let args = arguments(rest, |flag| {
        matches!(
            flag,
            "--keep-env"
                | "--cmode"
                | "--escape"
                | "--description"
                | "--target-storage"
                | "--bwlimit"
                | "--timeout"
                | "--foreground"
                | "--agent"
                | "--storage"
        )
    });
    if args.pending.is_some()
        || args
            .switches
            .iter()
            .any(|flag| !matches!(*flag, "--force" | "--online" | "--delete" | "--synchronous"))
    {
        return vec![];
    }
    let node = if let (Some(cluster), [id, ..]) = (cluster, args.positional.as_slice()) {
        let Some((_, host)) = cluster.owner(tool, action, id, true) else {
            return vec![];
        };
        Endpoint::ssh(host)
    } else {
        node
    };
    match (action, args.positional.as_slice()) {
        (_, []) => guests(node, tool, action, current),
        ("rollback" | "delsnapshot", [id]) if vmid(id) => snapshots(node, tool, id),
        ("migrate", [id]) if vmid(id) => nodes(node),
        (
            "move-volume" | "move_volume" | "move-disk" | "move_disk" | "importdisk" | "importovf",
            [id, _],
        ) if vmid(id) => storage(node, ""),
        _ => vec![],
    }
}
