//! Explicit cluster entry point and node routing for Proxmox guest commands.
use super::{
    guest_addresses, host_name, metadata, node_command, running_only, vmid, Endpoint, Rows,
};
use crate::util::{config_home, open_regular};
use serde_json::Value;
use std::collections::{BTreeMap, HashSet};
use std::io::Read;

pub(super) struct Cluster {
    seed: String,
    owners: BTreeMap<String, String>,
}

pub(super) fn configured(hosts: &[String]) -> BTreeMap<String, Cluster> {
    let Ok(file) = open_regular(config_home().join("complete-me/proxmox-clusters")) else {
        return BTreeMap::new();
    };
    let mut contents = String::new();
    if file.take(65_537).read_to_string(&mut contents).is_err() || contents.len() > 65_536 {
        return BTreeMap::new();
    }
    let allowed: HashSet<&str> = hosts.iter().map(String::as_str).collect();
    let mut clusters = BTreeMap::new();
    let mut declared = HashSet::new();
    for line in contents.lines() {
        let fields = line
            .split('#')
            .next()
            .unwrap_or_default()
            .split_whitespace()
            .collect::<Vec<_>>();
        let [name, seed, nodes @ ..] = fields.as_slice() else {
            continue;
        };
        if !host_name(name) || allowed.contains(name) {
            continue;
        }
        // A duplicate entry is ambiguous, including a later malformed definition.
        if !declared.insert(*name) {
            clusters.remove(*name);
            continue;
        }
        if !allowed.contains(seed) || nodes.is_empty() {
            continue;
        }
        let mut owners = BTreeMap::new();
        if nodes.iter().all(|mapping| {
            mapping.split_once('=').is_some_and(|(node, alias)| {
                host_name(node)
                    && allowed.contains(alias)
                    && owners.insert(node.into(), alias.into()).is_none()
            })
        }) {
            clusters.insert(
                (*name).into(),
                Cluster {
                    seed: (*seed).into(),
                    owners,
                },
            );
        }
    }
    clusters
}

struct Guest {
    id: String,
    node: String,
    name: String,
    state: String,
    kind: String,
}

impl Cluster {
    fn inventory(&self, completing: bool) -> Vec<Guest> {
        let output = Endpoint::ssh(&self.seed).request(
            "pvesh",
            &[
                "get",
                "/cluster/resources",
                "--type",
                "vm",
                "--output-format",
                "json",
            ],
            "cluster guest ID",
            completing,
        );
        let rows: Vec<Value> = serde_json::from_str(&output).unwrap_or_default();
        let mut ids = HashSet::new();
        let mut duplicate_ids = HashSet::new();
        let guests: Vec<_> = rows
            .iter()
            .filter_map(|row| {
                let id = crate::live::field(row, "vmid");
                if !vmid(&id) {
                    return None;
                }
                if !ids.insert(id.clone()) {
                    duplicate_ids.insert(id.clone());
                }
                Some(Guest {
                    id,
                    node: crate::live::field(row, "node"),
                    name: crate::live::field(row, "name"),
                    state: crate::live::field(row, "status"),
                    kind: crate::live::field(row, "type"),
                })
            })
            .collect();
        guests
            .into_iter()
            .filter(|guest| !duplicate_ids.contains(&guest.id))
            .collect()
    }

    fn alias(&self, guest: &Guest, tool: &str, action: &str) -> Option<&str> {
        let kind = if tool == "pct" { "lxc" } else { "qemu" };
        (guest.kind == kind && (!running_only(tool, action) || guest.state == "running"))
            .then(|| self.owners.get(&guest.node))
            .flatten()
            .map(String::as_str)
    }

    pub(super) fn candidates(&self, tool: &str, action: &str, prefix: &str) -> Rows {
        let mut rows: Vec<_> = self
            .inventory(true)
            .into_iter()
            .filter_map(|guest| {
                let alias = self.alias(&guest, tool, action)?;
                if !guest.id.starts_with(prefix) {
                    return None;
                }
                let description =
                    metadata(&[guest.name.clone(), guest.state.clone(), guest.node.clone()]);
                Some((guest, alias, description))
            })
            .collect();
        if rows.len() == 1 && std::env::var("COMPLETE_ME_PROXMOX_IP").as_deref() == Ok("1") {
            let (guest, alias, desc) = &mut rows[0];
            if tool == "pct" || guest.state == "running" {
                let ips = guest_addresses(Endpoint::ssh(alias), tool, &guest.id);
                if !ips.is_empty() {
                    desc.push_str(" · ");
                    desc.push_str(&ips.join(", "));
                }
            }
        }
        rows.into_iter()
            .map(|(guest, _, description)| (guest.id, description))
            .collect()
    }

    pub(super) fn owner(
        &self,
        tool: &str,
        action: &str,
        id: &str,
        completing: bool,
    ) -> Option<(&str, &str)> {
        let guest = self
            .inventory(completing)
            .into_iter()
            .find(|guest| guest.id == id)?;
        let (node, _) = self.owners.get_key_value(&guest.node)?;
        Some((node, self.alias(&guest, tool, action)?))
    }

    pub(super) fn node_alias(&self, node: &str) -> Option<&str> {
        self.owners.get(node).map(String::as_str)
    }

    pub(super) fn node_candidates(&self) -> Rows {
        self.owners
            .iter()
            .map(|(node, alias)| (node.clone(), format!("SSH via {alias}")))
            .collect()
    }

    pub(super) fn run(&self, args: &[String]) -> i32 {
        if let [option, node, rest @ ..] = args {
            if option == "--node" {
                if let Some(alias) = self.node_alias(node) {
                    return node_command(alias, Some(node), rest);
                }
                eprintln!("complete-me: choose a configured Proxmox node");
                return 2;
            }
        }
        let [tool, action, tail @ ..] = args else {
            eprintln!("complete-me: expected CLUSTER pct|qm ACTION GUEST-ID");
            return 2;
        };
        let Some((action, rest)) = super::guest_action(tool, action, tail) else {
            eprintln!("complete-me: choose a node alias for commands without a guest target");
            return 2;
        };
        let Some(id) = rest.first().filter(|value| vmid(value)) else {
            eprintln!("complete-me: place the guest ID immediately after its action");
            return 2;
        };
        let Some((node, host)) = self.owner(tool, action, id, false) else {
            eprintln!("complete-me: no unique eligible guest on a mapped cluster node");
            return 1;
        };
        node_command(host, Some(node), args)
    }
}
