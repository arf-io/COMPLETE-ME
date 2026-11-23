//! Namespace-aware Kubernetes selection with explicit execution scope.
use super::{common, field, metadata, query_with_timeout, Rows};
use serde_json::Value;
use std::os::unix::process::CommandExt;
use std::process::Command;
use std::time::Duration;

const POD_COMMANDS: &[&str] = &["logs", "exec", "attach", "port-forward"];
const RESOURCE_COMMANDS: &[&str] = &["get", "describe"];
const KUBECTL_SCOPE_FLAGS: &[&str] = &[
    "--namespace",
    "-n",
    "--all-namespaces",
    "-A",
    "--context",
    "--kubeconfig",
    "--server",
    "-s",
    "--cluster",
    "--user",
    "--token",
    "--username",
    "--password",
    "--as",
    "--as-group",
    "--as-uid",
    "--certificate-authority",
    "--client-certificate",
    "--client-key",
    "--insecure-skip-tls-verify",
    "--tls-server-name",
    "--kuberc",
];

fn context_name(name: &str) -> bool {
    name.as_bytes()
        .first()
        .is_some_and(u8::is_ascii_alphanumeric)
        && name.len() <= 512
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._:/@-".contains(&b))
}

fn label(name: &str) -> bool {
    let valid_end = |b: &u8| b.is_ascii_lowercase() || b.is_ascii_digit();
    name.len() <= 63
        && name.as_bytes().first().is_some_and(valid_end)
        && name.as_bytes().last().is_some_and(valid_end)
        && name.bytes().all(|b| valid_end(&b) || b == b'-')
}

pub(super) fn resource_name(name: &str) -> bool {
    name.len() <= 253 && name.split('.').all(label)
}

fn selector_override(words: &[String]) -> bool {
    words.iter().take_while(|word| *word != "--").any(|word| {
        KUBECTL_SCOPE_FLAGS.iter().any(|flag| {
            word == flag
                || word.strip_prefix(flag).is_some_and(|suffix| {
                    if flag.starts_with("--") {
                        suffix.starts_with('=')
                    } else {
                        !suffix.starts_with('-')
                    }
                })
        }) || word.starts_with('-')
            && !word.starts_with("--")
            && word
                .trim_start_matches('-')
                .chars()
                .any(|flag| matches!(flag, 'n' | 's' | 'A'))
    })
}

fn lookup(context: &str, args: &[&str]) -> Value {
    let scope = vec![format!("--context={context}")];
    let output = query_with_timeout("kubectl", &scope, args, Duration::from_millis(950)).0;
    serde_json::from_str(&output).unwrap_or_default()
}

fn namespaces(context: &str) -> Rows {
    let value = lookup(
        context,
        &["get", "namespaces", "--request-timeout=750ms", "-o", "json"],
    );
    std::iter::once((
        "@all".into(),
        "Search all namespaces with a qualified name".into(),
    ))
    .chain(
        value["items"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|item| {
                let name = field(&item["metadata"], "name");
                label(&name).then(|| {
                    (
                        name,
                        metadata(&[
                            "Kubernetes namespace".into(),
                            field(&item["status"], "phase"),
                        ]),
                    )
                })
            }),
    )
    .collect()
}

fn namespaced_kind(kind: &str) -> Option<&'static str> {
    common::kube_kind(kind).filter(|kind| !matches!(*kind, "nodes" | "namespaces"))
}

fn commands(all: bool) -> Rows {
    let mut rows: Rows = POD_COMMANDS
        .iter()
        .chain(RESOURCE_COMMANDS)
        .map(|cmd| ((*cmd).into(), "kubectl command".into()))
        .collect();
    if !all {
        rows.extend(
            ["apply", "delete", "edit", "rollout", "scale"]
                .into_iter()
                .map(|cmd| (cmd.into(), "kubectl command".into())),
        );
    }
    rows
}

pub(super) fn containers(scope: &[String], pod: &str, operation: &str, prefix: &str) -> Rows {
    let pod = pod
        .strip_prefix("pod/")
        .or_else(|| pod.strip_prefix("pods/"))
        .unwrap_or(pod);
    if !resource_name(pod) {
        return vec![];
    }
    let output = query_with_timeout(
        "kubectl",
        scope,
        &["get", "pod", pod, "--request-timeout=600ms", "-o", "json"],
        Duration::from_millis(800),
    )
    .0;
    let value: Value = serde_json::from_str(&output).unwrap_or_default();
    let collections: &[(&str, &str)] = if operation == "logs" {
        &[
            ("containers", "container"),
            ("initContainers", "init container"),
            ("ephemeralContainers", "ephemeral container"),
        ]
    } else {
        &[("containers", "container")]
    };
    collections
        .iter()
        .flat_map(|(key, kind)| {
            value["spec"][key]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(move |item| {
                    let name = field(item, "name");
                    resource_name(&name).then(|| {
                        (
                            format!("{prefix}{name}"),
                            metadata(&[(*kind).into(), field(item, "image")]),
                        )
                    })
                })
        })
        .collect()
}

fn qualified(context: &str, kind: &str, running: bool) -> Rows {
    let value = lookup(
        context,
        &[
            "get",
            kind,
            "--all-namespaces",
            "--request-timeout=750ms",
            "-o",
            "json",
        ],
    );
    value["items"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let ns = field(&item["metadata"], "namespace");
            let name = field(&item["metadata"], "name");
            let state = field(&item["status"], "phase");
            if !label(&ns) || !resource_name(&name) || running && state != "Running" {
                return None;
            }
            Some((
                format!("{ns}/{name}"),
                metadata(&[
                    kind.into(),
                    state,
                    field(&item["spec"], "nodeName"),
                    field(&item["status"], "podIP"),
                ]),
            ))
        })
        .collect()
}

pub(super) fn candidates(words: &[String], current: &str) -> Rows {
    match words {
        [] => common::kubectl(&["--context".into()], current)
            .into_iter()
            .filter(|(context, _)| context_name(context))
            .collect(),
        [context] if context_name(context) => namespaces(context),
        [context, namespace, rest @ ..]
            if context_name(context) && (namespace == "@all" || label(namespace)) =>
        {
            if rest.is_empty() {
                return commands(namespace == "@all");
            }
            if selector_override(rest) {
                return vec![];
            }
            if namespace == "@all" {
                if let [cmd, qualified, flags @ ..] = rest {
                    if matches!(cmd.as_str(), "logs" | "exec" | "attach") {
                        let parsed = super::arguments(flags, |f| matches!(f, "--container" | "-c"));
                        let container_slot = parsed.pending.map(|flag| (flag, 0)).or_else(|| {
                            super::value_option(current, |f| matches!(f, "--container" | "-c"))
                                .filter(|&(_, offset)| {
                                    offset < current.len() || current.ends_with('=')
                                })
                        });
                        if let (true, Some((_, offset)), Some((ns, pod))) =
                            (parsed.options, container_slot, qualified.split_once('/'))
                        {
                            if label(ns) && resource_name(pod) {
                                return containers(
                                    &[format!("--context={context}"), format!("--namespace={ns}")],
                                    pod,
                                    cmd,
                                    &current[..offset],
                                );
                            }
                        }
                    }
                }
                if current.starts_with('-') {
                    return vec![];
                }
                return match rest {
                    [cmd] if POD_COMMANDS.contains(&cmd.as_str()) => {
                        qualified(context, "pods", cmd != "logs")
                    }
                    [cmd] if RESOURCE_COMMANDS.contains(&cmd.as_str()) => common::kube_kinds()
                        .iter()
                        .filter_map(|kind| namespaced_kind(kind))
                        .map(|kind| (kind.into(), "Namespaced resource".into()))
                        .collect(),
                    [cmd, kind] if RESOURCE_COMMANDS.contains(&cmd.as_str()) => {
                        namespaced_kind(kind)
                            .map_or_else(Vec::new, |kind| qualified(context, kind, false))
                    }
                    _ => vec![],
                };
            }
            let mut scoped = vec![
                format!("--context={context}"),
                format!("--namespace={namespace}"),
            ];
            scoped.extend_from_slice(rest);
            common::kubectl(&scoped, current)
        }
        _ => vec![],
    }
}

fn routed_args(args: &[String]) -> Option<(String, Vec<String>)> {
    let (at, after) = match args {
        [cmd, qualified, tail @ ..] if POD_COMMANDS.contains(&cmd.as_str()) => {
            (1, (qualified, tail))
        }
        [cmd, kind, qualified, tail @ ..]
            if RESOURCE_COMMANDS.contains(&cmd.as_str()) && namespaced_kind(kind).is_some() =>
        {
            (2, (qualified, tail))
        }
        _ => return None,
    };
    let (namespace, name) = after.0.split_once('/')?;
    if !label(namespace) || !resource_name(name) {
        return None;
    }
    // Get/describe accept several resource arguments; restrict this routing to one.
    if at == 2 && !after.1.is_empty() {
        return None;
    }
    let mut result = args.to_vec();
    result[at] = name.into();
    Some((namespace.into(), result))
}

pub(crate) fn run(args: &[String]) -> i32 {
    let [context, namespace, rest @ ..] = args else {
        eprintln!("usage: complete-me kube CONTEXT NAMESPACE|@all COMMAND …");
        return 2;
    };
    if !context_name(context)
        || rest.is_empty()
        || rest[0].starts_with('-')
        || selector_override(rest)
    {
        eprintln!("complete-me: provide one context and namespace before the kubectl command");
        return 2;
    }
    let (namespace, args) = if namespace == "@all" {
        let Some(routed) = routed_args(rest) else {
            eprintln!("complete-me: use a supported command with one namespace/name target");
            return 2;
        };
        routed
    } else if label(namespace) {
        (namespace.clone(), rest.to_vec())
    } else {
        eprintln!("complete-me: invalid Kubernetes namespace");
        return 2;
    };
    let error = Command::new("kubectl")
        .arg(format!("--context={context}"))
        .arg(format!("--namespace={namespace}"))
        .args(&args)
        .exec();
    eprintln!("complete-me: could not start kubectl: {error}");
    1
}
