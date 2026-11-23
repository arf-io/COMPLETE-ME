//! Live values in container options, including comma-separated specifications.
use super::containers::{list_containers, list_images, list_objects, named_resources};
use super::{query, Rows};

fn affix(rows: Rows, prefix: &str, suffix: &str) -> Rows {
    rows.into_iter().map(|(value, desc)| (format!("{prefix}{value}{suffix}"), desc)).collect()
}

fn from_inventory(tool: &str, scope: &[String], kind: &str) -> Rows {
    match kind {
        "container" => list_containers(tool, scope, false),
        "image" => list_images(tool, scope, true),
        "network" | "volume" => list_objects(tool, scope, kind, ""),
        _ => named_resources(tool, scope, kind).unwrap_or_default(),
    }
}

fn filter(tool: &str, scope: &[String], group: &str, action: &str, value: &str) -> Rows {
    let keys: &[(&str, &str)] = match (group, action) {
        ("", "ps") | ("container", "ls" | "list" | "ps") =>
            &[("ancestor", "image"), ("network", "network"), ("volume", "volume"), ("before", "container"), ("since", "container"), ("name", "container"), ("id", "container")],
        ("", "images") | ("image", "ls" | "list") =>
            &[("reference", "image"), ("before", "image"), ("since", "image")],
        ("network", "ls" | "list") => &[("name", "network")],
        ("volume", "ls" | "list") => &[("name", "volume")],
        ("pod", "ps" | "ls" | "list") if tool == "podman" => &[("name", "pod")],
        ("secret", "ls" | "list") => &[("name", "secret")],
        ("service", "ls" | "list") if tool == "docker" => &[("name", "service")],
        _ => return vec![],
    };
    let Some((key, fragment)) = value.split_once('=') else {
        return keys.iter().map(|(key, kind)| (format!("{key}="), format!("Match a {kind}"))).collect();
    };
    let Some((_, kind)) = keys.iter().find(|(name, _)| *name == key) else { return vec![] };
    if fragment.contains('/') && !matches!(*kind, "image") { return vec![] }
    affix(from_inventory(tool, scope, kind), &format!("{key}="), "")
}

fn mount(tool: &str, scope: &[String], value: &str) -> Rows {
    let mut fields: Vec<_> = value.split(',').collect();
    let fragment = fields.pop().unwrap_or_default();
    let prefix = &value[..value.len() - fragment.len()];
    let source_type = fields.iter().rev().find_map(|field| field.strip_prefix("type="));
    if let Some((key, name)) = fragment.split_once('=') {
        if key == "type" {
            return ["bind", "volume", "tmpfs"].into_iter().map(|kind| (
                format!("{prefix}type={kind}"), "Mount type".into()
            )).collect();
        }
        if matches!(key, "source" | "src") && matches!(source_type, None | Some("volume"))
            && !name.contains('/') && !name.starts_with(['.', '~'])
        {
            return affix(from_inventory(tool, scope, "volume"), &format!("{prefix}{key}="), "");
        }
        return vec![];
    }
    ["type=", "source=", "target=", "readonly"].into_iter()
        .filter(|field| !fields.iter().any(|old| old.starts_with(field)))
        .map(|key| (format!("{prefix}{key}"), "Mount field".into())).collect()
}

fn restart(tool: &str, scope: &[String], target: Option<&str>) -> Rows {
    if tool == "docker" {
        if let Some(name) = target.filter(|name| crate::probe::safe_name(name)) {
            let result = query(tool, scope, &["container", "inspect", "--format",
                "{{.HostConfig.RestartPolicy.Name}}|{{.HostConfig.RestartPolicy.MaximumRetryCount}}", "--", name]);
            if let Some((policy, count)) = result.trim_end().split_once('|') {
                if matches!(policy, "no" | "always" | "on-failure" | "unless-stopped") {
                    let value = if policy == "on-failure" && count.parse::<u32>().is_ok_and(|n| n > 0) {
                        format!("{policy}:{count}")
                    } else {
                        policy.into()
                    };
                    return vec![(value, "Current restart policy".into())];
                }
            }
        }
    }
    ["no", "always", "on-failure", "unless-stopped"].into_iter().map(|value| (
        value.into(), "Restart policy".into()
    )).collect()
}

pub(super) fn candidates(
    tool: &str, scope: &[String], (group, action): (&str, &str), args: &super::Arguments<'_>,
    flag: &str, current: &str, offset: usize,
) -> Rows {
    let value = &current[offset..];
    let create = matches!(action, "run" | "create") && matches!(group, "" | "container");
    let rows = match flag {
        "--filter" | "-f" | "f" => filter(tool, scope, group, action, value),
        "--type" | "-t" | "t" if action == "inspect" && group.is_empty() => {
            let mut kinds = vec!["container", "image", "network", "volume"];
            if tool == "docker" {
                kinds.extend(["node", "service", "plugin", "secret", "config"]);
            } else {
                kinds.extend(["pod", "all"]);
            }
            kinds.into_iter().map(|kind| (kind.into(), "Inspect object type".into())).collect()
        }
        "--network" if create || group == "service" && action == "create" => {
            if let Some(fragment) = value.strip_prefix("container:") {
                if !fragment.contains([':', '/']) {
                    return affix(list_containers(tool, scope, true), &format!("{}container:", &current[..offset]), "");
                }
            }
            if !value.contains([':', '/']) && !value.starts_with('.') {
                return affix(from_inventory(tool, scope, "network"), &current[..offset], "");
            }
            vec![]
        }
        "--network-add" | "--network-rm" if group == "service" && action == "update" =>
            from_inventory(tool, scope, "network"),
        "--volume" | "-v" | "v" if create && !value.contains([':', '/']) && !value.starts_with(['.', '~']) =>
            affix(from_inventory(tool, scope, "volume"), "", ":"),
        "--mount" if create || group == "service" && action == "create" => mount(tool, scope, value),
        "--mount-add" if group == "service" && action == "update" => mount(tool, scope, value),
        "--volumes-from" | "--link" if create && (flag != "--link" || tool == "docker") && !value.contains(':') =>
            list_containers(tool, scope, false),
        "--pod" if create && tool == "podman" && !value.starts_with("new:") =>
            from_inventory(tool, scope, "pod"),
        "--secret" if create && tool == "podman" && !value.contains(',') =>
            from_inventory(tool, scope, "secret"),
        "--secret" | "--secret-add" | "--config" | "--config-add" if tool == "docker" && group == "service" && matches!(action, "create" | "update") => {
            let (prefix, name) = value.strip_prefix("source=").map_or(("", value), |name| ("source=", name));
            if name.contains([',', '=']) { return vec![] }
            let kind = if flag.starts_with("--secret") { "secret" } else { "config" };
            affix(from_inventory(tool, scope, kind), prefix, "")
        }
        "--secret-rm" | "--config-rm" if tool == "docker" && group == "service" && action == "update" =>
            from_inventory(tool, scope, if flag == "--secret-rm" { "secret" } else { "config" }),
        "--restart" if create || action == "update" && matches!(group, "" | "container") =>
            restart(tool, scope, args.positional.first().copied().filter(|_| !create && args.positional.len() == 1)),
        _ => vec![],
    };
    affix(rows, &current[..offset], "")
}
