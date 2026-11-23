use super::{arguments, global, metadata, query, Rows};

const DOCKER_SCOPE: &[&str] = &[
    "--context",
    "-c",
    "--host",
    "-H",
    "--config",
    "--tlscacert",
    "--tlscert",
    "--tlskey",
    "--tls",
    "--tlsverify",
];
const DOCKER_GLOBAL: &[&str] = &[
    "--context",
    "-c",
    "--host",
    "-H",
    "--config",
    "--log-level",
    "-l",
    "--tlscacert",
    "--tlscert",
    "--tlskey",
];
const PODMAN_GLOBAL: &[&str] = &["--connection", "-c", "--url", "--identity", "--root", "--runroot", "--storage-driver", "--storage-opt", "--log-level", "--module", "--out", "--tmpdir", "--volumepath"];
const PODMAN_SCOPE: &[&str] = &["--connection", "-c", "--url", "--identity", "--root", "--runroot", "--remote", "-r"];
const DOCKER_VALUES: &[&str] = &[
    "--env",
    "-e",
    "--env-file",
    "--workdir",
    "-w",
    "--user",
    "-u",
    "--name",
    "--entrypoint",
    "--platform",
    "--label",
    "-l",
    "--label-file",
    "--mount",
    "--mount-add",
    "--volume",
    "-v",
    "--volumes-from",
    "--link",
    "--pod",
    "--secret",
    "--secret-add",
    "--secret-rm",
    "--config",
    "--config-add",
    "--config-rm",
    "--publish",
    "-p",
    "--expose",
    "--network",
    "--network-add",
    "--network-rm",
    "--network-alias",
    "--hostname",
    "-h",
    "--add-host",
    "--dns",
    "--dns-option",
    "--dns-search",
    "--ip",
    "--ip6",
    "--mac-address",
    "--device",
    "--device-cgroup-rule",
    "--security-opt",
    "--cap-add",
    "--cap-drop",
    "--group-add",
    "--pid",
    "--ipc",
    "--uts",
    "--userns",
    "--cgroupns",
    "--restart",
    "--pull",
    "--stop-signal",
    "--stop-timeout",
    "--timeout",
    "-t",
    "--signal",
    "-s",
    "--cpus",
    "--cpu-shares",
    "--cpuset-cpus",
    "--cpuset-mems",
    "--memory",
    "-m",
    "--memory-swap",
    "--memory-reservation",
    "--shm-size",
    "--ulimit",
    "--pids-limit",
    "--gpus",
    "--tmpfs",
    "--log-driver",
    "--log-opt",
    "--health-cmd",
    "--health-interval",
    "--health-timeout",
    "--health-start-period",
    "--health-start-interval",
    "--health-retries",
    "--runtime",
    "--annotation",
    "--since",
    "--until",
    "--tail",
    "-n",
    "--format",
    "-f",
    "--type",
    "--filter",
    "--change",
    "-c",
    "--author",
    "-a",
    "--message",
    "--output",
    "-o",
    "--detach-keys",
];

fn global_flags(tool: &str) -> (&'static [&'static str], &'static [&'static str]) {
    if tool == "podman" {
        (PODMAN_GLOBAL, PODMAN_SCOPE)
    } else {
        (DOCKER_GLOBAL, DOCKER_SCOPE)
    }
}

fn alternate_podman_storage(flag: &str) -> bool {
    matches!(flag, "--storage-driver" | "--storage-opt" | "--module" | "--tmpdir" | "--volumepath")
}

pub(super) fn candidates(tool: &str, words: &[String], current: &str) -> Rows {
    let (flags, forwarded) = global_flags(tool);
    let command = arguments(words, |flag| flags.contains(&flag));
    if command.positional.is_empty() && command.options {
        let slot = command.pending.map(|flag| (flag, 0)).or_else(|| {
            crate::context::value_option(current, |flag| flags.contains(&flag))
                .filter(|(_, offset)| *offset < current.len() || current.ends_with('='))
        });
        if let Some((flag, offset)) = slot {
            if matches!(flag, "-c" | "c" | "--context" | "--connection") {
                if tool == "podman" && command.values.iter().any(|(flag, _)| alternate_podman_storage(flag)) {
                    return vec![];
                }
                let config = command.values.iter().filter(|(flag, _)| *flag == "--config")
                    .map(|(_, value)| format!("--config={value}")).collect::<Vec<_>>();
                return connections(tool, &config, &current[..offset]);
            }
        }
    }
    let Some((scope, target)) = global(words, flags, forwarded) else {
        return vec![];
    };
    let leading = &words[..words.len() - target.len()];
    let mut rows = if tool == "podman" && arguments(leading, |flag| flags.contains(&flag))
        .values.iter().any(|(flag, _)| alternate_podman_storage(flag))
    {
        super::record(|| serde_json::json!({"source": tool, "status": "scope-blocked", "reason": "alternate-storage"}));
        vec![]
    } else {
        runtime(tool, &scope, target, current)
    };
    if !rows.iter().any(|(value, _)| value.starts_with(current)) {
        rows.extend(super::cli_help::candidates(tool, target, current));
    }
    rows
}

fn connections(tool: &str, scope: &[String], prefix: &str) -> Rows {
    let (command, label) = if tool == "docker" {
        (&["context", "ls", "--format", "{{.Name}}|{{.Current}}"][..], "Docker context")
    } else {
        (&["system", "connection", "list", "--format", "{{.Name}}|{{.Default}}"][..], "Podman connection")
    };
    let scope: Vec<String> = scope.iter().filter(|arg| arg.starts_with("--config=")).cloned().collect();
    query(tool, &scope, command).lines().filter_map(|row| {
        let (name, active) = row.split_once('|')?;
        crate::probe::safe_name(name).then(|| (
            format!("{prefix}{name}"),
            if active == "true" { format!("{label} · current") } else { label.into() },
        ))
    }).collect()
}

fn runtime(tool: &str, scope: &[String], words: &[String], current: &str) -> Rows {
    let Some(first) = words.first().map(String::as_str) else {
        return vec![];
    };
    if tool == "docker" && first == "compose" {
        return super::projects::compose(tool, scope, &words[1..], current);
    }
    if tool == "docker" && first == "context" || tool == "podman" && first == "system"
        && words.get(1).is_some_and(|word| word == "connection")
    {
        let action_index = if tool == "docker" { 1 } else { 2 };
        if let Some(action) = words.get(action_index) {
            let tail = &words[action_index + 1..];
            let args = arguments(tail, |flag| matches!(flag, "--format" | "-f" | "--description" | "--docker"));
            if args.positional.is_empty() && args.pending.is_none() && !current.starts_with('-') &&
                (tool == "docker" && matches!(action.as_str(), "use" | "inspect" | "rm" | "remove" | "update" | "export")
                    || tool == "podman" && matches!(action.as_str(), "default" | "remove" | "rm" | "rename"))
            {
                return connections(tool, scope, "");
            }
        }
    }
    let (namespace, action, rest) = if matches!(first, "container" | "image" | "network" | "volume" |
        "config" | "node" | "plugin" | "secret" | "service" | "stack" | "pod" | "machine") {
        let Some(action) = words.get(1) else {
            return vec![];
        };
        (first, action.as_str(), &words[2..])
    } else {
        ("", first, &words[1..])
    };
    let takes = |flag: &str| match flag {
        "-t" => matches!(action, "restart" | "stop") || tool == "podman" && namespace.is_empty() && action == "inspect",
        "-a" => matches!(action, "commit" | "run" | "create"),
        "-c" => matches!(action, "commit" | "run" | "create"),
        "-h" => matches!(action, "run" | "create"),
        "-f" => matches!(action, "inspect" | "stats" | "images" | "ls" | "list" | "ps"),
        "-s" => action == "kill",
        "-l" => matches!(action, "run" | "create"),
        _ => DOCKER_VALUES.contains(&flag),
    };
    let args = arguments(rest, takes);
    let pos = args.positional.len();
    let slot = args.pending.map(|flag| (flag, 0)).or_else(|| {
        crate::context::value_option(current, takes)
            .filter(|(_, offset)| *offset < current.len() || current.ends_with('='))
    });
    if let Some((flag, offset)) = slot {
        // Podman and Docker expose option values between operands as well as
        // before them. A container command after IMAGE/CONTAINER owns its argv.
        if matches!(action, "run" | "create" | "exec") && pos > 0 {
            return vec![];
        }
        return super::container_options::candidates(tool, scope, (namespace, action), &args, flag, current, offset);
    }
    if args.options && current.starts_with('-') {
        return vec![];
    }
    if matches!(namespace, "network" | "volume")
        && (matches!(action, "inspect" | "rm" | "remove" | "exists")
            || (namespace == "network" && matches!(action, "connect" | "disconnect") && pos == 0))
    {
        return list_objects(tool, scope, namespace, "");
    }
    if let Some(action) = words.get(1) {
        let target = arguments(&words[2..], |flag| matches!(flag, "--format" | "-f" | "--filter" | "--signal" | "-s" | "--timeout" | "-t" | "--replicas"));
        if target.pending.is_none() && (target.positional.is_empty() || matches!(action.as_str(), "rm" | "remove" | "inspect" | "stop" | "start" | "restart" | "kill" | "pause" | "unpause" | "scale")) {
            if let Some(rows) = resource(tool, scope, first, action) {
                return if action == "scale" {
                    rows.into_iter().map(|(name, desc)| (format!("{name}="), desc)).collect()
                } else { rows };
            }
        }
    }
    let kind = args.values.iter().rev().find(|(flag, _)| matches!(*flag, "--type" | "-t" | "t") && action == "inspect").map(|(_, v)| *v);
    if namespace.is_empty() && action == "inspect" {
        if let Some(kind) = kind {
            if matches!(kind, "network" | "volume") {
                return list_objects(tool, scope, kind, "");
            }
            if kind != "machine" {
                if let Some(rows) = named_resources(tool, scope, kind) {
                    return rows;
                }
            }
        }
    }
    let (containers, images, running) = match (namespace, action) {
        ("image", "inspect" | "remove" | "rm" | "save" | "exists") | ("", "rmi" | "save") => (false, true, false),
        ("image", "history" | "tag" | "push")
        | ("", "history" | "tag" | "push" | "run" | "create")
        | ("container", "run" | "create") => (false, pos == 0, false),
        ("", "inspect") => (
            kind.is_none_or(|k| matches!(k, "container" | "all")),
            kind.is_none_or(|k| matches!(k, "image" | "all")),
            false,
        ),
        ("container", "inspect" | "exists") => (true, false, false),
        ("network", "connect" | "disconnect") => (pos == 1, false, false),
        ("container" | "", "start" | "restart" | "rm" | "remove" | "wait" | "stats" | "update") => (true, false, false),
        ("container" | "", "stop" | "kill" | "pause" | "unpause") => (true, false, true),
        ("container" | "", "attach" | "exec" | "top") => (pos == 0, false, true),
        ("container" | "", "logs" | "rename" | "commit" | "diff" | "export" | "port" | "clone" | "restore" | "checkpoint") => (pos == 0, false, false),
        ("container" | "", "cp") => (pos < 2 && !current.contains([':', '/']), false, false),
        _ => return vec![],
    };
    let mut rows = vec![];
    if containers {
        let suffix = if action == "cp" { ":" } else { "" };
        rows.extend(list_containers(tool, scope, running).into_iter()
            .map(|(name, desc)| (format!("{name}{suffix}"), desc)));
    }
    if images {
        rows.extend(list_images(tool, scope, action != "push"));
    }
    rows
}

pub(super) fn list_containers(tool: &str, scope: &[String], running: bool) -> Rows {
    let mut command = vec!["container", "ls"];
    if !running {
        command.push("--all");
    }
    command.extend(["--format", "{{.Names}}|{{.ID}}|{{.Image}}|{{.Status}}"]);
    query(tool, scope, &command).lines().flat_map(|row| {
        let fields: Vec<_> = row.splitn(4, '|').map(str::to_owned).collect();
        let [name, id, image, status] = fields.as_slice() else { return vec![] };
        let desc = metadata(&[image.clone(), status.clone(), id.clone()]);
        [name, id].into_iter().filter(|value| !value.is_empty())
            .map(|value| (value.clone(), desc.clone())).collect()
    }).collect()
}

pub(super) fn list_images(tool: &str, scope: &[String], include_ids: bool) -> Rows {
    let command = ["image", "ls", "--format", "{{.Repository}}|{{.Tag}}|{{.ID}}|{{.Size}}|{{.CreatedSince}}"];
    let mut rows = vec![];
    for row in query(tool, scope, &command).lines() {
        let fields: Vec<_> = row.splitn(5, '|').map(str::to_owned).collect();
        let [repo, tag, id, size, age] = fields.as_slice() else { continue };
        let reference = format!("{repo}:{tag}");
        if !matches!(repo.as_str(), "<none>" | "") && !matches!(tag.as_str(), "<none>" | "") {
            rows.push((reference.clone(), metadata(&[id.clone(), size.clone(), age.clone()])));
        }
        if include_ids && !id.is_empty() {
            rows.push((id.clone(), metadata(&[reference, size.clone(), age.clone()])));
        }
    }
    rows
}

fn resource(tool: &str, scope: &[String], group: &str, action: &str) -> Option<Rows> {
    let valid_action = match (tool, group) {
        ("docker", "plugin") => matches!(action, "inspect" | "enable" | "disable" | "rm" | "remove" | "set" | "upgrade"),
        ("docker", "config" | "secret") => matches!(action, "inspect" | "rm" | "remove"),
        ("docker", "service") => matches!(action, "inspect" | "logs" | "ps" | "rm" | "remove" | "update" | "rollback" | "scale"),
        ("docker", "stack") => matches!(action, "services" | "ps" | "rm" | "remove"),
        ("docker", "node") => matches!(action, "inspect" | "ps" | "rm" | "remove" | "update" | "promote" | "demote"),
        ("podman", "pod") => matches!(action, "inspect" | "logs" | "top" | "rm" | "start" | "stop" | "restart" | "kill" | "pause" | "unpause" | "exists"),
        ("podman", "secret") => matches!(action, "inspect" | "rm" | "exists"),
        ("podman", "machine") => matches!(action, "inspect" | "start" | "stop" | "rm" | "set" | "ssh"),
        _ => false,
    };
    valid_action.then(|| named_resources(tool, scope, group)).flatten()
}

pub(super) fn named_resources(tool: &str, scope: &[String], group: &str) -> Option<Rows> {
    let (command, format, kind) = match (tool, group) {
        ("docker", "plugin") => ("ls", "{{.Name}}|{{.Enabled}}", "Docker plugin"),
        ("docker", "config" | "secret") => ("ls", "{{.Name}}|{{.ID}}", group),
        ("docker", "service") => ("ls", "{{.Name}}|{{.Replicas}}", "Swarm service"),
        ("docker", "stack") => ("ls", "{{.Name}}|{{.Services}}", "Swarm stack"),
        ("docker", "node") => ("ls", "{{.Hostname}}|{{.Status}}", "Swarm node"),
        ("podman", "pod") => ("ps", "{{.Name}}|{{.Status}}", "Podman pod"),
        ("podman", "secret") => ("ls", "{{.Name}}|{{.ID}}", "Podman secret"),
        ("podman", "machine") => ("list", "{{.Name}}|{{.Running}}", "Podman machine"),
        _ => return None,
    };
    let scope = if group == "machine" { &[] } else { scope };
    Some(query(tool, scope, &[group, command, "--format", format]).lines().filter_map(|row| {
        let (name, detail) = row.split_once('|')?;
        (!name.is_empty()).then(|| (name.into(), metadata(&[kind.into(), detail.into()])))
    }).collect())
}

pub(super) fn list_objects(tool: &str, scope: &[String], object: &str, prefix: &str) -> Rows {
    let format = if object == "volume" {
        "{{.Name}}|{{.Driver}}"
    } else {
        "{{.Name}}|{{.Driver}}|{{.ID}}"
    };
    query(tool, scope, &[object, "ls", "--format", format])
        .lines()
        .filter_map(|row| {
            let (name, desc) = row.split_once('|')?;
            Some((format!("{prefix}{name}"), desc.replace('|', " · ")))
        })
        .collect()
}
