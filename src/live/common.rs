use super::{arguments, field, metadata, query, same_flag, value_option, Rows};
use serde_json::Value;

const KUBE_SCOPE: &[&str] = &["--context", "--namespace", "-n"];
const KUBE_OTHER_SCOPE: &[&str] = &[
    "--kubeconfig",
    "--server",
    "-s",
    "--cluster",
    "--user",
    "--token",
    "--as",
    "--as-group",
    "--as-uid",
    "--certificate-authority",
    "--client-certificate",
    "--client-key",
    "--insecure-skip-tls-verify",
    "--tls-server-name",
];
const KUBE_VALUES: &[&str] = &[
    "--output",
    "-o",
    "--selector",
    "-l",
    "--field-selector",
    "--filename",
    "-f",
    "--kustomize",
    "-k",
    "--container",
    "-c",
    "--tail",
    "--since",
    "--since-time",
    "--request-timeout",
    "--timeout",
    "--sort-by",
];

pub(super) fn kube_kinds() -> &'static [&'static str] {
    &["pods", "services", "deployments", "daemonsets", "statefulsets", "namespaces", "nodes",
        "jobs", "cronjobs", "configmaps", "ingresses", "persistentvolumeclaims"]
}

pub(super) fn kube_kind(kind: &str) -> Option<&'static str> {
    Some(match kind {
        "pod" | "pods" | "po" => "pods",
        "service" | "services" | "svc" => "services",
        "deployment" | "deployments" | "deploy" => "deployments",
        "daemonset" | "daemonsets" | "ds" => "daemonsets",
        "statefulset" | "statefulsets" | "sts" => "statefulsets",
        "namespace" | "namespaces" | "ns" => "namespaces",
        "node" | "nodes" | "no" => "nodes",
        "job" | "jobs" => "jobs",
        "cronjob" | "cronjobs" | "cj" => "cronjobs",
        "configmap" | "configmaps" | "cm" => "configmaps",
        "ingress" | "ingresses" | "ing" => "ingresses",
        "persistentvolumeclaim" | "persistentvolumeclaims" | "pvc" => "persistentvolumeclaims",
        _ => return None,
    })
}

pub(super) fn kubectl(words: &[String], current: &str) -> Rows {
    let takes = |f: &str| KUBE_SCOPE.contains(&f) || KUBE_OTHER_SCOPE.contains(&f) || KUBE_VALUES.contains(&f);
    let args = arguments(words, takes);
    if !args.options
        || words.iter().any(|w| {
            KUBE_OTHER_SCOPE.iter().any(|f| {
                w == f
                    || w.strip_prefix(f)
                        .is_some_and(|s| s.starts_with('=') || !f.starts_with("--"))
            })
        })
    {
        return vec![];
    }
    let slot = args.pending.map(|flag| (flag, 0)).or_else(|| {
        value_option(current, takes).filter(|(_, offset)| *offset < current.len() || current.ends_with('='))
    });
    let context_command = matches!(
        args.positional.as_slice(),
        [
            "config",
            "use-context" | "delete-context" | "rename-context" | "get-contexts"
        ]
    );
    if slot.is_some_and(|(f, _)| f == "--context") || (slot.is_none() && context_command && !current.starts_with('-')) {
        let offset = slot.map_or(0, |(_, offset)| offset);
        return query("kubectl", &[], &["config", "get-contexts", "-o", "name"])
            .lines()
            .map(|name| (format!("{}{name}", &current[..offset]), "Kubernetes context".into()))
            .collect();
    }
    let mut scope = vec![];
    for (flag, value) in &args.values {
        if *flag == "--context" {
            scope.push(format!("--context={value}"));
        } else if same_flag(flag, "-n") || *flag == "--namespace" {
            scope.push(format!("--namespace={value}"));
        }
    }
    if let Some((flag, offset)) = slot {
        if same_flag(flag, "-c") || flag == "--container" {
            if let ["logs" | "exec" | "attach", pod] = args.positional.as_slice() {
                if !words.iter().any(|word| matches!(word.as_str(), "-A" | "--all-namespaces")) {
                    return super::kube::containers(&scope, pod, args.positional[0], &current[..offset]);
                }
            }
            return vec![];
        }
    }
    let (kind, prefix) = if let Some((flag, offset)) = slot {
        if !same_flag(flag, "-n") && flag != "--namespace" {
            return vec![];
        }
        ("namespaces", &current[..offset])
    } else {
        if current.starts_with('-') {
            return vec![];
        }
        match args.positional.as_slice() {
            ["get" | "describe" | "edit" | "delete" | "label" | "annotate", kind] => {
                let Some(kind) = kube_kind(kind) else {
                    return vec![];
                };
                (kind, "")
            }
            ["logs" | "exec" | "attach" | "port-forward" | "get" | "describe" | "edit" | "delete"] => {
                if let Some((kind, _)) = current.split_once('/') {
                    let Some(resource) = kube_kind(kind) else {
                        return vec![];
                    };
                    (resource, &current[..=kind.len()])
                } else if matches!(args.positional[0], "logs" | "exec" | "attach" | "port-forward") {
                    ("pods", "")
                } else {
                    return vec![];
                }
            }
            _ => return vec![],
        }
    };
    let mut command = vec!["get", kind, "--request-timeout=300ms", "-o", "json"];
    if words.iter().any(|w| matches!(w.as_str(), "-A" | "--all-namespaces")) {
        // An unqualified name may select a different resource in the current namespace.
        return vec![];
    }
    // Match selectors already specified for this command.
    let selectors: Vec<_> = args
        .values
        .iter()
        .filter_map(|(f, v)| match *f {
            "-l" | "l" | "--selector" => Some(format!("--selector={v}")),
            "--field-selector" => Some(format!("--field-selector={v}")),
            _ => None,
        })
        .collect();
    command.extend(selectors.iter().map(String::as_str));
    let output = query("kubectl", &scope, &command);
    let Ok(value) = serde_json::from_str::<Value>(&output) else {
        return vec![];
    };
    value["items"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let name = field(&item["metadata"], "name");
            (!name.is_empty()).then(|| {
                (
                    format!("{prefix}{name}"),
                    metadata(&[
                        kind.into(),
                        field(&item["metadata"], "namespace"),
                        field(&item["status"], "phase"),
                    ]),
                )
            })
        })
        .collect()
}

pub(super) fn systemd(tool: &str, words: &[String], current: &str) -> Rows {
    let takes = |f: &str| {
        matches!(
            f,
            "-u" | "--unit"
                | "--user-unit"
                | "-t"
                | "--type"
                | "--state"
                | "-H"
                | "--host"
                | "-M"
                | "--machine"
                | "--root"
                | "--image"
                | "-p"
                | "--property"
                | "-n"
                | "--lines"
                | "--since"
                | "--until"
        )
    };
    let args = arguments(words, takes);
    if !args.options
        || args.values.iter().any(|(f, _)| {
            matches!(
                *f,
                "-H" | "H" | "--host" | "-M" | "M" | "--machine" | "--root" | "--image"
            )
        })
    {
        return vec![];
    }
    let (prefix, user_unit) = if tool == "journalctl" {
        let Some((flag, offset)) = args.pending.map(|f| (f, 0)).or_else(|| {
            value_option(current, takes).filter(|(_, offset)| *offset < current.len() || current.ends_with('='))
        }) else {
            return vec![];
        };
        if !same_flag(flag, "-u") && !matches!(flag, "--unit" | "--user-unit") {
            return vec![];
        }
        (&current[..offset], flag == "--user-unit")
    } else {
        if args.pending.is_some()
            || current.starts_with('-')
            || !matches!(
                args.positional.first(),
                Some(
                    &("start"
                        | "stop"
                        | "restart"
                        | "reload"
                        | "status"
                        | "is-active"
                        | "is-enabled"
                        | "enable"
                        | "disable"
                        | "mask"
                        | "unmask"
                        | "cat"
                        | "show"
                        | "reset-failed"
                        | "try-restart"
                        | "reload-or-restart")
                )
            )
        {
            return vec![];
        }
        ("", false)
    };
    let scope = if user_unit || words.iter().any(|w| w == "--user") {
        vec!["--user".into()]
    } else {
        vec![]
    };
    let listing = if tool == "systemctl"
        && matches!(
            args.positional.first(),
            Some(&("enable" | "disable" | "is-enabled" | "mask" | "unmask" | "start" | "cat"))
        ) {
        "list-unit-files"
    } else {
        "list-units"
    };
    query(
        "systemctl",
        &scope,
        &[listing, "--all", "--plain", "--no-legend", "--no-pager"],
    )
    .lines()
    .filter_map(|row| {
        let row = row.trim();
        let (name, state) = row.split_once(char::is_whitespace)?;
        (name.contains('.') && !name.ends_with('.')).then(|| {
            (
                format!("{prefix}{name}"),
                state.split_whitespace().collect::<Vec<_>>().join(" "),
            )
        })
    })
    .collect()
}

pub(super) fn package_scripts(tool: &str, words: &[String], current: &str) -> Rows {
    use std::io::Read;
    let directory_flags = &["--prefix", "--dir", "-C", "--cwd"];
    let args = arguments(words, |flag| directory_flags.contains(&flag));
    if args.pending.is_some()
        || !args.options
        || current.starts_with(['-', '/'])
        || !matches!(args.positional.as_slice(), ["run" | "run-script"])
        || words.iter().any(|word| {
            matches!(
                word.split('=').next().unwrap_or(word),
                "--filter"
                    | "-F"
                    | "--workspace"
                    | "-w"
                    | "--workspaces"
                    | "--recursive"
                    | "-r"
                    | "--workspace-root"
                    | "-T"
                    | "--top-level"
                    | "-B"
                    | "--binaries-only"
            )
        })
    {
        return vec![];
    }
    let Ok(cwd) = std::env::current_dir() else {
        return vec![];
    };
    let root = args
        .values
        .iter()
        .rev()
        .find(|(f, _)| directory_flags.iter().any(|flag| same_flag(f, flag)))
        .map(|(_, dir)| cwd.join(dir));
    let manifest = match root {
        Some(dir) => dir.join("package.json"),
        None => {
            let Some(manifest) = cwd
                .ancestors()
                .map(|dir| dir.join("package.json"))
                .find(|path| path.is_file())
            else {
                return vec![];
            };
            manifest
        }
    };
    if !manifest.is_file() {
        return vec![];
    }
    let mut contents = Vec::new();
    if std::fs::File::open(manifest)
        .and_then(|file| file.take(400_000).read_to_end(&mut contents))
        .is_err()
    {
        return vec![];
    }
    let Ok(package) = serde_json::from_slice::<Value>(&contents) else {
        return vec![];
    };
    super::record(|| serde_json::json!({"source": "package.json", "operation": "scripts", "status": "success"}));
    let description = metadata(&["package.json script".into(), field(&package, "name")]);
    package["scripts"]
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(name, script)| {
            script.is_string() && !name.starts_with('-') && (tool != "pnpm" || !name.starts_with('.'))
        })
        .map(|(name, _)| (name.clone(), description.clone()))
        .collect()
}

pub(super) fn brew(words: &[String], current: &str) -> Rows {
    let [action, rest @ ..] = words else { return vec![] };
    if current.starts_with('-')
        || !matches!(
            action.as_str(),
            "uninstall"
                | "remove"
                | "rm"
                | "reinstall"
                | "upgrade"
                | "info"
                | "list"
                | "ls"
                | "link"
                | "unlink"
                | "pin"
                | "unpin"
        )
    {
        return vec![];
    }
    let cask_only = rest.iter().any(|w| w == "--cask");
    let formula_only =
        matches!(action.as_str(), "link" | "unlink" | "pin" | "unpin") || rest.iter().any(|w| w == "--formula");
    let mut rows = vec![];
    if !cask_only {
        let formulae = query("brew", &[], &["list", "--versions", "--formula"]);
        rows.extend(formulae.lines().filter_map(|row| {
            let (name, versions) = row.split_once(' ')?;
            (!versions.is_empty()).then(|| (name.into(), format!("Installed formula · {versions}")))
        }));
    }
    if !formula_only {
        // A stale cask receipt can fail the full --versions listing.
        let casks = query("brew", &[], &["list", "--cask"]);
        rows.extend(casks.lines().map(|name| (name.into(), "Installed cask".into())));
    }
    rows
}
