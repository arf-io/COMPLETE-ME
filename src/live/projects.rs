use super::{arguments, field, global, metadata, query, record, same_flag, value_option, Rows};
use serde_json::Value;

const COMPOSE_OPTIONS: &[&str] = &[
    "-f",
    "--file",
    "-p",
    "--project-name",
    "--project-directory",
    "--env-file",
    "--profile",
    "--ansi",
    "--progress",
    "--parallel",
];
const COMPOSE_SCOPE: &[&str] = &[
    "-f",
    "--file",
    "-p",
    "--project-name",
    "--project-directory",
    "--env-file",
    "--profile",
];

pub(super) fn compose(tool: &str, docker_scope: &[String], words: &[String], current: &str) -> Rows {
    let Some((scope, rest)) = global(words, COMPOSE_OPTIONS, COMPOSE_SCOPE) else {
        let args = arguments(words, |flag| COMPOSE_OPTIONS.contains(&flag));
        if args.pending != Some("--profile") {
            return vec![];
        }
        let Some((scope, _)) = global(&words[..words.len() - 1], COMPOSE_OPTIONS, COMPOSE_SCOPE) else {
            return vec![];
        };
        return compose_values(tool, docker_scope, &scope, "--profiles", "", "Compose profile");
    };
    if rest.is_empty() {
        if let Some((_, offset)) = value_option(current, |flag| flag == "--profile") {
            if current.contains('=') {
                return compose_values(
                    tool,
                    docker_scope,
                    &scope,
                    "--profiles",
                    &current[..offset],
                    "Compose profile",
                );
            }
        }
        return vec![];
    }
    let action = rest[0].as_str();
    let takes = |flag: &str| match flag {
        "-t" => matches!(action, "up" | "down" | "stop" | "restart"),
        "-e" | "-u" | "-w" => matches!(action, "run" | "exec"),
        "-n" => action == "logs",
        "-p" => action == "run",
        _ => matches!(
            flag,
            "--timeout"
                | "--tail"
                | "--since"
                | "--until"
                | "--scale"
                | "--pull"
                | "--wait-timeout"
                | "--exit-code-from"
                | "--attach"
                | "--no-attach"
                | "--build-arg"
                | "--name"
                | "--entrypoint"
                | "--user"
                | "--workdir"
                | "--env"
                | "--publish"
                | "--volume"
                | "--index"
                | "--format"
                | "--status"
                | "--filter"
                | "--env-from-file"
                | "--env-file"
                | "--network"
                | "--platform"
        ),
    };
    let args = arguments(&rest[1..], takes);
    let slot = args.pending.map(|flag| (flag, 0)).or_else(|| value_option(current, takes)
        .filter(|(_, offset)| *offset < current.len() || current.ends_with('=')));
    if let Some((flag, offset)) = slot {
        let suffix = match (action, flag) {
            ("up" | "create", "--scale") if !current[offset..].contains('=') => "=",
            ("up", "--exit-code-from" | "--attach" | "--no-attach") => "",
            _ => return vec![],
        };
        return compose_values(tool, docker_scope, &scope, "--services", &current[..offset], "Compose service")
            .into_iter().map(|(name, desc)| (format!("{name}{suffix}"), desc)).collect();
    }
    if args.options && current.starts_with('-') || matches!(action, "run" | "exec" | "port") && !args.positional.is_empty() {
        return vec![];
    }
    if action == "cp" && args.positional.len() < 2 && !current.contains([':', '/'])
        && !current.starts_with(['.', '~'])
    {
        return compose_values(tool, docker_scope, &scope, "--services", "", "Compose service path")
            .into_iter().map(|(name, desc)| (format!("{name}:"), desc)).collect();
    }
    if !matches!(
        action,
        "up" | "down"
            | "build"
            | "config"
            | "create"
            | "events"
            | "exec"
            | "kill"
            | "logs"
            | "pause"
            | "port"
            | "ps"
            | "pull"
            | "push"
            | "restart"
            | "rm"
            | "run"
            | "start"
            | "stop"
            | "top"
            | "unpause"
            | "wait"
            | "watch"
            | "images"
    ) {
        return vec![];
    }
    compose_values(tool, docker_scope, &scope, "--services", "", "Compose service")
}

fn compose_values(
    tool: &str,
    docker_scope: &[String],
    project: &[String],
    switch: &str,
    prefix: &str,
    desc: &str,
) -> Rows {
    let mut scope = docker_scope.to_vec();
    if tool == "docker" {
        scope.push("compose".into());
    }
    // Profiles are names from the config. Include every profile when listing
    // service targets because explicitly naming a service activates its profile.
    scope.extend(project.iter().cloned());
    if switch == "--services" {
        scope.push("--profile=*".into());
    }
    query(tool, &scope, &["config", switch, "--no-env-resolution"])
        .lines()
        .filter(|name| name.bytes().all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b)))
        .map(|name| (format!("{prefix}{name}"), desc.into()))
        .collect()
}

pub(super) fn tasks(tool: &str, words: &[String], current: &str) -> Rows {
    if current.starts_with('-') || words.iter().any(|w| w == "--") {
        return vec![];
    }
    let flags = match tool {
        "just" => &["-f", "--justfile", "-d", "--working-directory"][..],
        _ => &["-t", "--taskfile", "-d", "--dir", "--global", "-g"][..],
    };
    let valued: Vec<_> = flags
        .iter()
        .copied()
        .filter(|f| !matches!(*f, "-g" | "--global"))
        .collect();
    let Some((scope, rest)) = global(words, &valued, flags) else {
        return vec![];
    };
    if (tool == "just" && !rest.is_empty()) || rest.iter().any(|w| w.starts_with('-') || w.contains('=')) {
        return vec![];
    }
    if tool == "just" {
        return query(tool, &scope, &["--list", "--list-heading=", "--list-prefix="])
            .lines()
            .filter_map(|line| {
                let (entry, description) = line.split_once('#').unwrap_or((line, ""));
                let name = entry.split_whitespace().next()?;
                Some((name.into(), description.trim().into()))
            })
            .collect();
    }
    taskfile(&scope)
}

fn taskfile(scope: &[String]) -> Rows {
    use std::io::Read;
    let mut base = match std::env::current_dir() {
        Ok(path) => path,
        Err(_) => return vec![],
    };
    let mut explicit = None;
    let mut iter = scope.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--global" | "-g" => base = crate::util::home(),
            "-d" => {
                if let Some(dir) = iter.next() {
                    base = base.join(dir);
                }
            }
            "-t" => explicit = iter.next().map(String::as_str),
            _ => {
                if let Some(dir) = arg.strip_prefix("--dir=") {
                    base = base.join(dir);
                } else if let Some(path) = arg.strip_prefix("--taskfile=") {
                    explicit = Some(path);
                }
            }
        }
    }
    const NAMES: &[&str] = &[
        "Taskfile.yml",
        "Taskfile.yaml",
        "taskfile.yml",
        "taskfile.yaml",
        "Taskfile.dist.yml",
        "Taskfile.dist.yaml",
    ];
    let path = if let Some(path) = explicit {
        base.join(path)
    } else {
        let Some(path) = base
            .ancestors()
            .flat_map(|dir| NAMES.iter().map(move |name| dir.join(name)))
            .find(|path| path.is_file())
        else {
            return vec![];
        };
        path
    };
    let mut text = String::new();
    if !path.is_file()
        || std::fs::File::open(path)
            .and_then(|file| file.take(400_000).read_to_string(&mut text))
            .is_err()
    {
        return vec![];
    }
    let Ok(docs) = yaml_rust2::YamlLoader::load_from_str(&text) else {
        record(|| serde_json::json!({"source": "Taskfile", "operation": "tasks", "status": "invalid-output"}));
        return vec![];
    };
    record(|| serde_json::json!({"source": "Taskfile", "operation": "tasks", "status": "success"}));
    let mut rows = vec![];
    for doc in &docs {
        let Some(tasks) = doc["tasks"].as_hash() else { continue };
        for (name, task) in tasks {
            let Some(name) = name.as_str() else { continue };
            if task["internal"].as_bool() == Some(true) {
                continue;
            }
            let desc = task["desc"].as_str().unwrap_or("Taskfile task");
            rows.push((name.into(), desc.into()));
            if let Some(aliases) = task["aliases"].as_vec() {
                rows.extend(
                    aliases
                        .iter()
                        .filter_map(|alias| alias.as_str())
                        .map(|alias| (alias.into(), format!("Alias for {name}"))),
                );
            }
        }
    }
    rows
}

pub(super) fn github(words: &[String], current: &str) -> Rows {
    if current.starts_with('-') || current.starts_with("http") {
        return vec![];
    }
    let takes = |f: &str| {
        matches!(
            f,
            "--repo"
                | "-R"
                | "--json"
                | "--jq"
                | "-q"
                | "--template"
                | "-t"
                | "--body"
                | "-b"
                | "--body-file"
                | "-F"
                | "--subject"
                | "--author"
                | "--reason"
                | "--hostname"
                | "--head"
                | "--base"
                | "--branch"
                | "-B"
        )
    };
    let parsed = arguments(words, takes);
    if !parsed.options || parsed.pending.is_some() || parsed.positional.len() != 2 {
        return vec![];
    }
    let kind = parsed.positional[0];
    let action = parsed.positional[1];
    let state = match (kind, action) {
        ("pr", "view" | "diff" | "checks" | "checkout" | "comment" | "edit")
        | ("issue", "view" | "comment" | "edit") => "all",
        ("pr", "close" | "merge" | "review" | "ready") | ("issue", "close" | "pin" | "unpin") => "open",
        ("pr" | "issue", "reopen") => "closed",
        _ => return vec![],
    };

    let repo = parsed
        .values
        .iter()
        .rev()
        .find(|(flag, _)| *flag == "--repo" || same_flag(flag, "-R"))
        .map(|(_, repo)| repo.to_string())
        .or_else(|| super::effective_env("gh", "GH_REPO").and_then(|value| value.into_string().ok()));
    if repo.as_deref().is_some_and(|repo| !valid_repo(repo)) {
        return vec![];
    }
    let scope = repo.map(|repo| vec![format!("--repo={repo}")]).unwrap_or_default();
    let args = [
        kind,
        "list",
        "--state",
        state,
        "--limit",
        "50",
        "--json",
        "number,title,state,updatedAt",
    ];
    let data = super::cache::github(&scope, &args);
    let Ok(list) = serde_json::from_str::<Vec<Value>>(&data) else {
        if !data.is_empty() {
            record(|| serde_json::json!({"source": "gh", "status": "invalid-output"}));
        }
        return vec![];
    };
    list.iter()
        .filter_map(|item| {
            let number = item["number"].as_u64()?;
            Some((
                number.to_string(),
                metadata(&[field(item, "title"), field(item, "state"), field(item, "updatedAt")]),
            ))
        })
        .collect()
}

fn valid_repo(repo: &str) -> bool {
    let parts: Vec<_> = repo.split('/').collect();
    (2..=3).contains(&parts.len())
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b)))
}
