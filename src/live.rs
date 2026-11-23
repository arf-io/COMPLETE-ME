//! Read-only completion providers. User input selects a slot and connection scope;
//! each provider constructs its own query instead of executing the command being edited.
use crate::context::value_option;
use crate::scope::Scope;
use crate::util::{query_result_scoped, Args, QueryStatus};
use serde_json::Value;
use std::collections::HashSet;
use std::time::{Duration, Instant};

pub(crate) mod agents;
pub(crate) mod bytes;
mod cache;
mod cli_help;
pub(crate) mod cloud;
mod common;
mod container_options;
mod containers;
mod coverage;
mod devices;
mod git;
mod git_config;
pub(crate) mod kube;
mod network;
mod projects;
pub(crate) mod proxmox;
mod tmux;
pub(crate) mod zfs;

pub(crate) use tmux::gather as gather_tmux;

pub(crate) fn git_config_words(words: &[String]) -> Option<&[String]> {
    let (tool, rest) = words.split_first()?;
    (tool.rsplit('/').next() == Some("git")).then(|| git::config_words(rest)).flatten()
}

pub(crate) fn git_config_value_slot(config_words: &[String], current: &str) -> bool {
    git_config::value_slot(config_words, current)
}

pub type Rows = Vec<(String, String)>;

thread_local! {
    static ACTIVE_SCOPE: std::cell::RefCell<Scope> = std::cell::RefCell::new(Scope::default());
    static TRACE: std::cell::RefCell<Option<Vec<Value>>> = const { std::cell::RefCell::new(None) };
}

fn record(event: impl FnOnce() -> Value) {
    TRACE.with_borrow_mut(|trace| {
        if let Some(trace) = trace {
            trace.push(event());
        }
    });
}

fn effective_env(tool: &str, key: &str) -> Option<std::ffi::OsString> {
    ACTIVE_SCOPE.with_borrow(|scope| scope.effective(tool, key))
}

fn remote_disabled() -> bool {
    std::env::var_os("COMPLETE_ME_NO_REMOTE").is_some_and(|value| !value.is_empty() && value != "0")
}

fn permits_query(tool: &str, scope: &[String], args: &[&str]) -> bool {
    if !remote_disabled() {
        return true;
    }
    let permitted = match tool {
        "gh" | "vultr-cli" => false,
        "aws" => matches!(args, ["configure", "list-profiles", ..]),
        "kubectl" => args.first() == Some(&"config"),
        // These two inventories are local CLI configuration, even when the
        // default endpoint is remote. They never connect to a daemon.
        "docker" if matches!(args, ["context", "ls", ..]) => true,
        "podman" if matches!(args, ["system", "connection", "list", ..]) => true,
        "podman" if matches!(args, ["machine", "list", ..]) => true,
        "docker" | "podman" | "docker-compose" => {
            let endpoint = if tool == "podman" {
                "CONTAINER_HOST"
            } else {
                "DOCKER_HOST"
            };
            let context = if tool == "podman" {
                "CONTAINER_CONNECTION"
            } else {
                "DOCKER_CONTEXT"
            };
            let env_endpoint = effective_env(tool, endpoint);
            let explicit_endpoint = scope
                .windows(2)
                .filter(|pair| pair[0] == "-H")
                .map(|pair| pair[1].as_str())
                .chain(
                    scope
                        .iter()
                        .filter_map(|arg| arg.strip_prefix("--host=").or_else(|| arg.strip_prefix("--url="))),
                )
                .last();
            let endpoint = explicit_endpoint.or_else(|| env_endpoint.as_deref().and_then(std::ffi::OsStr::to_str));
            let named_context = effective_env(tool, context)
                .is_some_and(|value| !value.is_empty() && value != "default")
                || scope.iter().any(|arg| {
                    matches!(arg.as_str(), "-c" | "--remote" | "-r")
                        || arg.starts_with("--connection=")
                        || arg.strip_prefix("--context=").is_some_and(|name| name != "default")
                });
            // Compose files can reference remote includes, so offline mode keeps
            // ordinary file completion for Compose instead of loading a project.
            let compose = tool == "docker-compose" || scope.iter().any(|arg| arg == "compose");
            !compose && !named_context && endpoint.is_some_and(|address| address.starts_with("unix://"))
        }
        _ => true,
    };
    if !permitted {
        record(|| serde_json::json!({"source": tool, "status": "remote-disabled"}));
    }
    permitted
}

fn query(tool: &str, scope: &[String], args: &[&str]) -> String {
    query_with_timeout(tool, scope, args, Duration::from_millis(350)).0
}

fn query_with_timeout(tool: &str, scope: &[String], args: &[&str], timeout: Duration) -> (String, QueryStatus) {
    let argv = std::iter::once(tool.to_string())
        .chain(scope.iter().cloned())
        .chain(args.iter().map(|s| s.to_string()))
        .collect::<Vec<_>>();
    if !permits_query(tool, scope, args) {
        return (String::new(), QueryStatus::Success);
    }
    let start = Instant::now();
    let mut overrides = ACTIVE_SCOPE.with_borrow(|active| active.for_tool(tool));
    let selector_count = overrides.len();
    // Object and tree queries must not fetch missing objects from promisor remotes.
    if tool == "git" {
        overrides.extend([("GIT_NO_LAZY_FETCH".into(), Some("1".into())), ("GIT_ALLOW_PROTOCOL".into(), Some(String::new()))]);
    }
    if matches!(tool, "hf" | "huggingface-cli") {
        overrides.extend([("HF_HUB_OFFLINE".into(), Some("1".into())), ("HF_HUB_DISABLE_TELEMETRY".into(), Some("1".into()))]);
    } else if tool == "aws" {
        overrides.extend([
            ("AWS_MAX_ATTEMPTS".into(), Some("1".into())),
            ("AWS_IGNORE_CONFIGURED_ENDPOINT_URLS".into(), Some("true".into())),
        ]);
    }
    let (output, outcome) = query_result_scoped(&argv, timeout, &overrides);
    let elapsed = start.elapsed().as_millis() as u64;
    record(|| {
        // Show the read-only operation and recognized selectors. Full argv and
        // stderr may contain credentials, so they are excluded from reports.
        let action = args
            .iter()
            .filter(|arg| !arg.starts_with('-'))
            .take(match tool {
                "docker" | "podman" | "kubectl" | "gh" => {
                    if args.first() == Some(&"config") && tool == "docker" {
                        1
                    } else {
                        2
                    }
                }
                "docker-compose" => 1,
                _ => 1,
            })
            .copied()
            .collect::<Vec<_>>()
            .join(" ");
        let selected: Vec<_> = scope
            .iter()
            .filter(|arg| {
                ["--context=", "--namespace=", "--connection=", "--repo="]
                    .iter()
                    .any(|flag| arg.starts_with(flag))
            })
            .map(|s| crate::render::clean_desc(s))
            .collect();
        let slot = match (tool, args) {
            ("git", ["worktree", "list", ..]) => Some("Git worktree"),
            ("git", ["--no-pager", "stash", "list", ..]) => Some("Git stash"),
            ("git", ["ls-files", ..]) => Some("Git worktree path"),
            ("git", ["ls-tree", ..]) => Some("Git revision path"),
            ("git", ["remote"]) => Some("Git remote"),
            ("git", ["for-each-ref", ..]) => Some("Git ref"),
            ("git", ["--no-pager", "config", "--null", "--get", ..]) => Some("current Git config value"),
            ("git", ["--no-pager", "config", "--null", "--name-only", ..]) => Some("Git config key"),
            ("docker", ["context", "ls", ..]) => Some("Docker context"),
            ("podman", ["system", "connection", "list", ..]) => Some("Podman connection"),
            ("podman", ["machine", "list", ..]) => Some("Podman machine"),
            ("docker", ["container", "inspect", ..]) => Some("current container restart policy"),
            ("docker" | "docker-compose", ["config", "--profiles", ..]) => Some("Compose profile"),
            ("docker" | "docker-compose", ["config", "--services", ..]) => Some("Compose service"),
            ("aws", ["s3api", "list-objects-v2", ..]) => Some("S3 object or prefix"),
            ("gh", ["pr", "list", ..]) => Some("pull request number"),
            ("gh", ["issue", "list", ..]) => Some("issue number"),
            ("tmux", ["-N", "list-sessions", ..]) => Some("tmux session"),
            ("tmux", ["-N", "list-windows", ..]) => Some("tmux window"),
            ("tmux", ["-N", "list-panes", ..]) => Some("tmux pane"),
            ("tmux", ["-N", "list-clients", ..]) => Some("tmux client"),
            _ => None,
        };
        let action = if tool == "docker" && scope.iter().any(|s| s == "compose") {
            format!("compose {action}")
        } else {
            action
        };
        let status = match outcome {
            QueryStatus::Success if output.trim().is_empty() => "empty",
            QueryStatus::Success => "success",
            QueryStatus::Missing => "tool-missing",
            QueryStatus::Timeout => "timeout",
            QueryStatus::Failed(_) => "failed",
        };
        serde_json::json!({
            "source": tool, "operation": action, "slot": slot, "scope": if scope.is_empty() && !args.iter().any(|arg| matches!(*arg, "--global" | "--system" | "--local" | "--worktree") || arg.starts_with("--file=")) { "current" } else { "explicit" },
            "selectors": selected, "status": status, "elapsed_ms": elapsed,
            "environment_selectors": overrides[..selector_count].iter().map(|(name, _)| name).collect::<Vec<_>>(),
            "exit_code": match outcome { QueryStatus::Failed(code) => code, _ => None },
        })
    });
    (output, outcome)
}

fn field(value: &Value, key: &str) -> String {
    match &value[key] {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => String::new(),
    }
}

fn metadata(fields: &[String]) -> String {
    fields
        .iter()
        .filter(|s| !s.is_empty())
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(" · ")
}

fn same_flag(actual: &str, flag: &str) -> bool {
    actual == flag || flag.strip_prefix('-') == Some(actual)
}

struct Arguments<'a> {
    positional: Vec<&'a str>,
    values: Vec<(&'a str, &'a str)>,
    switches: Vec<&'a str>,
    pending: Option<&'a str>,
    options: bool,
}

fn arguments<'a>(words: &'a [String], takes_value: impl Fn(&str) -> bool) -> Arguments<'a> {
    let mut out = Arguments {
        positional: vec![],
        values: vec![],
        switches: vec![],
        pending: None,
        options: true,
    };
    for word in words {
        if let Some(flag) = out.pending.take() {
            out.values.push((flag, word));
        } else if out.options && word == "--" {
            out.options = false;
        } else if out.options && word.starts_with('-') && word != "-" {
            if let Some((flag, offset)) = value_option(word, &takes_value) {
                if offset < word.len() || word.ends_with('=') {
                    out.values.push((flag, &word[offset..]));
                } else {
                    out.pending = Some(flag);
                }
            } else {
                out.switches.push(word);
            }
        } else {
            out.positional.push(word);
        }
    }
    out
}

/// Consume leading global switches and retain only the explicitly supported scope flags.
fn global<'a>(words: &'a [String], flags: &[&str], forward: &[&str]) -> Option<(Vec<String>, &'a [String])> {
    let mut i = 0;
    let mut scope = vec![];
    while let Some(word) = words.get(i).filter(|w| w.starts_with('-') && *w != "-") {
        i += 1;
        if word == "--" {
            break;
        }
        if let Some((flag, offset)) = value_option(word, |f| flags.contains(&f)) {
            let mut value = &word[offset..];
            if value.is_empty() && !word.ends_with('=') {
                let next = words.get(i)?;
                i += 1;
                value = next;
            }
            if let Some(flag) = forward.iter().find(|f| same_flag(flag, f)) {
                if flag.starts_with("--") {
                    scope.push(format!("{flag}={value}"));
                } else {
                    scope.extend([flag.to_string(), value.to_string()]);
                }
            }
        } else if forward.contains(&word.split('=').next().unwrap_or(word)) {
            scope.push(word.clone());
        }
    }
    Some((scope, &words[i..]))
}

fn zerotier(words: &[String], current: &str) -> Rows {
    // ZeroTier short flags have attached values. Keep token/port/home arguments
    // opaque and pass them only to the same CLI when a network ID is needed.
    let start = words.iter().position(|w| !w.starts_with('-')).unwrap_or(words.len());
    if words.len() != start + 1
        || current.starts_with('-')
        || !matches!(words[start].as_str(), "join" | "leave" | "get" | "set")
    {
        return vec![];
    }
    let scope: Vec<String> = words[..start]
        .iter()
        .filter(|w| w.len() > 2 && ["-D", "-p", "-T"].iter().any(|f| w.starts_with(f)))
        .cloned()
        .collect();
    let out = query("zerotier-cli", &scope, &["-j", "listnetworks"]);
    let networks: Vec<Value> = serde_json::from_str(&out).unwrap_or_default();
    networks
        .iter()
        .filter_map(|network| {
            let mut id = field(network, "nwid");
            if id.is_empty() {
                id = field(network, "id");
            }
            if id.len() != 16 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
                return None;
            }
            let addresses = network["assignedAddresses"]
                .as_array()
                .map(|a| a.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", "))
                .unwrap_or_default();
            Some((
                id,
                metadata(&[
                    field(network, "name"),
                    field(network, "status"),
                    field(network, "type"),
                    addresses,
                ]),
            ))
        })
        .collect()
}

pub fn candidates(words: &[String], current: &str) -> Rows {
    let (words, scope) = crate::scope::normalize(words.to_vec());
    scoped_candidates(&words, current, &scope)
}

pub(crate) fn scoped_candidates(words: &[String], current: &str, scope: &Scope) -> Rows {
    scoped_candidates_with_right(words, current, scope, "", false)
}

pub(crate) fn aws_object_slot(words: &[String], current: &str) -> bool {
    words.split_first().is_some_and(|(tool, rest)| {
        tool.rsplit('/').next() == Some("aws") && cloud::aws_object_slot(rest, current)
    })
}

pub(crate) fn restore_password_slot(words: &[String], current: &str) -> bool {
    let tool = words.first().and_then(|tool| tool.rsplit('/').next());
    let restore = match tool {
        Some("pct") => words.get(1).is_some_and(|word| matches!(word.as_str(), "restore" | "create")),
        Some("proxmox" | "complete-me") => words.windows(2).any(|pair| pair[0] == "pct" && matches!(pair[1].as_str(), "restore" | "create")),
        _ => false,
    };
    restore && words.iter().map(String::as_str).chain([current]).any(|word| {
        let flag = word.split('=').next().unwrap_or(word);
        matches!(flag, "-password" | "--password")
    })
}

pub(crate) fn scoped_candidates_with_right(words: &[String], current: &str, scope: &Scope, right: &str, fish: bool) -> Rows {
    let Some(tool) = words.first().map(|word| word.rsplit('/').next().unwrap_or(word)) else {
        return vec![];
    };
    if let Some(reason) = scope.blocker(tool) {
        record(|| serde_json::json!({"source": tool, "status": "scope-blocked", "reason": reason}));
        return vec![];
    }
    let previous = ACTIVE_SCOPE.replace(scope.clone());
    let rows = dispatch(words, current, right, fish);
    ACTIVE_SCOPE.replace(previous);
    rows
}

pub(crate) fn has_provider(tool: &str) -> bool {
    agents::known(tool) || matches!(
        tool,
        "tmux"
            | "docker"
            | "podman"
            | "zerotier-cli"
            | "git"
            | "kubectl" | "kube" | "aws" | "vultr-cli" | "hf" | "huggingface-cli" | "cm-cloud"
            | "systemctl"
            | "journalctl"
            | "npm"
            | "pnpm"
            | "yarn"
            | "bun"
            | "brew"
            | "docker-compose"
            | "gh"
            | "just"
            | "task"
            | "proxmox" | "pct" | "qm" | "pvesm" | "vzdump" | "qmrestore" | "zfs" | "zpool" | "zfs-path"
            | "dd" | "hexdump" | "tcpdump" | "tshark" | "wireshark" | "ip" | "route" | "nmap"
    )
}

fn dispatch(words: &[String], current: &str, right: &str, fish: bool) -> Rows {
    let Some((tool, rest)) = words.split_first() else {
        return vec![];
    };
    let tool = tool.rsplit('/').next().unwrap_or(tool);
    let rows = match tool {
        "tmux" => tmux::candidates(rest, current),
        "proxmox" => proxmox::shortcut(rest, current),
        "zfs-path" => zfs::path_candidates(rest, current),
        "dd" => devices::candidates(rest, current),
        "tcpdump" | "tshark" | "wireshark" | "ip" | "route" | "nmap" => network::candidates(tool, rest, current),
        "pct" | "qm" | "pvesm" | "vzdump" | "qmrestore" | "zfs" | "zpool" => {
            proxmox::candidates(proxmox::Endpoint::local(), tool, rest, current)
        }
        "complete-me" => match rest {
            [sub, args @ ..] if sub == "cloud" => cloud::api_candidates(args),
            [sub, args @ ..] if sub == "kube" => kube::candidates(args, current),
            [sub, args @ ..] if sub == "proxmox" => proxmox::shortcut(args, current),
            [sub, args @ ..] if sub == "zfs-path" => zfs::path_candidates(args, current),
            _ => vec![],
        },
        "docker" | "podman" => containers::candidates(tool, rest, current),
        "zerotier-cli" => zerotier(rest, current),
        "git" => git::candidates(rest, current),
        "kubectl" => common::kubectl(rest, current),
        "kube" => kube::candidates(rest, current),
        "aws" => cloud::aws(rest, current, right, fish),
        "vultr-cli" => cloud::vultr(rest, current),
        "hf" | "huggingface-cli" => cloud::hf(tool, rest, current),
        "cm-cloud" => cloud::api_candidates(rest),
        "systemctl" | "journalctl" => common::systemd(tool, rest, current),
        "npm" | "pnpm" | "yarn" | "bun" => common::package_scripts(tool, rest, current),
        "brew" => common::brew(rest, current),
        "docker-compose" => projects::compose(tool, &[], rest, current),
        "gh" => projects::github(rest, current),
        "just" | "task" => projects::tasks(tool, rest, current),
        _ => {
            let rows = agents::candidates(tool, rest, current);
            if rows.is_empty() && !agents::known(tool) {
                record(|| serde_json::json!({"source": "shell", "status": "unsupported-tool"}));
            }
            rows
        }
    };
    let mut seen = HashSet::new();
    rows.into_iter()
        .filter(|(value, _)| {
            !value.is_empty()
                && value.starts_with(current)
                && !value.chars().any(char::is_control)
                && seen.insert(value.clone())
        })
        .map(|(v, d)| (v, crate::render::clean_desc(&d)))
        .collect()
}

/// Fish provides its own unescaped tokens and reattaches any short-option prefix itself.
pub(crate) fn diagnose(words: &[String], current: &str, scope: &Scope) -> Value {
    diagnose_with_right(words, current, scope, "")
}

fn diagnose_with_right(words: &[String], current: &str, scope: &Scope, right: &str) -> Value {
    let start = Instant::now();
    TRACE.with_borrow_mut(|trace| *trace = Some(vec![]));
    let rows = scoped_candidates_with_right(words, current, scope, right, false);
    let events = TRACE.with_borrow_mut(|trace| trace.take().unwrap_or_default());
    let has_status = |status| events.iter().any(|event| event["status"] == status);
    let state = if !rows.is_empty() {
        "matches"
    } else if events.is_empty() {
        "no-provider-for-slot"
    } else if has_status("unsupported-tool") {
        "unsupported-tool"
    } else if has_status("scope-blocked") {
        "scope-blocked"
    } else if has_status("remote-disabled") {
        "remote-disabled"
    } else if has_status("credentials-unavailable") {
        "credentials-unavailable"
    } else if ["tool-missing", "timeout", "failed", "unreadable", "invalid-output"]
        .iter()
        .any(|status| has_status(*status))
    {
        "lookup-failed"
    } else {
        "no-matches"
    };
    let tool = words
        .first()
        .map(|word| word.rsplit('/').next().unwrap_or(word))
        .unwrap_or("");
    serde_json::json!({"status": state, "matches": rows.len(), "sources": events,
        "elapsed_ms": start.elapsed().as_millis() as u64, "runtime": scope.report(tool),
        "remote_lookups": !remote_disabled()})
}

pub fn run(args: &[String]) -> i32 {
    let args = Args::parse(args, &["--current", "--fish-option", "--line", "--right"]);
    if args.has("--coverage") {
        return coverage::run(&args.rest, args.has("--json"));
    }
    let current = args.value("--current").unwrap_or_default();
    let editor = args.has("--editor-input");
    let (line, right) = if editor {
        let Some((line, right)) = crate::util::editor_input(&mut std::io::stdin().lock()) else { return 2 };
        (Some(line), right)
    } else {
        (args.value("--line"), args.value("--right").unwrap_or_default())
    };
    let (words, current, scope, tail) = if let Some(left) = line {
        let line = crate::extras::split_line(&left);
        let tail = if editor {
            let token = crate::cursor::token(&line.raw, &right, true);
            &right[token.right_len..]
        } else {
            &right
        };
        (line.words, line.cur, line.scope, tail)
    } else {
        let (words, scope) = crate::scope::normalize(args.rest.clone());
        (words, current, scope, right.as_str())
    };
    if args.has("--fish-native") && aws_object_slot(&words, &current) {
        return 0;
    }
    if args.has("--fish-scoped") {
        if restore_password_slot(&words, &current) {
            println!("blocked");
            return 0;
        }
        let tool = words
            .first()
            .map(|word| word.rsplit('/').next().unwrap_or(word))
            .unwrap_or("");
        if !has_provider(tool) {
            return 0;
        }
        if scope.blocker(tool).is_some() {
            println!("blocked");
            return 0;
        }
        if tool == "hexdump" && words.last().is_some_and(|word| word == "-s") {
            let rows = bytes::hexdump_candidates(tail, &current, true);
            if !rows.is_empty() {
                println!("scoped");
                for (value, desc) in rows {
                    println!("{value}\t{desc}");
                }
            }
            return 0;
        }
        let config = git_config_words(&words);
        let s3_object = aws_object_slot(&words, &current);
        if scope.for_tool(tool).is_empty() && config.is_none() && !s3_object {
            return 0;
        }
        let rows = scoped_candidates_with_right(&words, &current, &scope, tail, true);
        if !rows.is_empty() {
            println!("scoped");
            for (value, desc) in rows {
                println!("{value}\t{desc}");
            }
        } else if s3_object || config.is_some_and(|words| git_config_value_slot(words, &current)) {
            println!("blocked");
        }
        return 0;
    }
    if args.has("--explain") {
        let report = diagnose_with_right(&words, &current, &scope, tail);
        if args.has("--json") {
            println!("{report}");
        } else {
            println!("Completion status  {}", field(&report, "status"));
            println!("Candidates         {}", report["matches"]);
            println!("Elapsed            {} ms", report["elapsed_ms"]);
            for event in report["sources"].as_array().into_iter().flatten() {
                let selectors = event["selectors"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(" ");
                println!(
                    "  {} {} [{}]  {} {}",
                    field(event, "source"),
                    field(event, "operation"),
                    selectors,
                    field(event, "status"),
                    field(event, "slot")
                );
            }
        }
        return 0;
    }
    let option = args.value("--fish-option");
    let strip = option
        .as_deref()
        .and_then(|flag| value_option(&current, |f| f == flag).map(|(_, offset)| &current[..offset]))
        .unwrap_or("");
    for (value, desc) in scoped_candidates_with_right(&words, &current, &scope, tail, true) {
        if let Some(value) = value.strip_prefix(strip) {
            println!("{value}\t{desc}");
        }
    }
    0
}
