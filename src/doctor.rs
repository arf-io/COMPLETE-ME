//! Report what the calling shell knows and inspect completion state without generating it.
use crate::{extras, probe, util};
use serde_json::{json, Value};

const HELP: &str = "usage: complete-me doctor [--shell fish|bash|zsh] [--command CMD | --line LEFT-OF-CURSOR] [--json]";

fn basename(name: &str) -> &str {
    name.rsplit('/').next().unwrap_or(name)
}

pub(crate) fn run(args: &[String]) -> i32 {
    let a = util::Args::parse(args, &["--shell", "--command", "--line", "--binding", "--handler"]);
    if a.has("--help") || a.has("-h") {
        println!("{HELP}");
        return 0;
    }
    let line = a.value("--line").map(|line| extras::split_line(&line));
    let cmd = a
        .value("--command")
        .or_else(|| line.as_ref().and_then(|line| line.words.first().cloned()));
    let name = cmd.as_deref().map(basename).unwrap_or("");
    if !name.is_empty() && !probe::safe_name(name) {
        eprintln!("complete-me doctor: invalid command name");
        return 2;
    }
    if a.has("--target-only") {
        println!("{name}");
        return 0;
    }
    let explicit_shell = a.value("--shell");
    let inferred_shell = std::env::var("SHELL").ok();
    let shell = explicit_shell
        .as_deref()
        .or_else(|| inferred_shell.as_deref().map(basename))
        .unwrap_or("unknown");
    if explicit_shell.is_some() && !matches!(shell, "fish" | "bash" | "zsh") {
        eprintln!("{HELP}");
        return 2;
    }
    let binding = status(a.value("--binding"), &["complete-me", "native", "custom", "unbound"]);
    let handler = status(
        a.value("--handler"),
        &["generated", "native", "live-adapter", "loader", "none"],
    );
    let cache_root = util::cache_home().join("complete-me");
    let file = match shell {
        "fish" => Some(cache_root.join("fish").join(format!("{name}.fish"))),
        "bash" => Some(cache_root.join("bash").join(name)),
        "zsh" => Some(cache_root.join("zsh").join(format!("{name}.zsh"))),
        _ => None,
    }
    .filter(|_| !name.is_empty());
    let cache = file.as_ref().map(|file| {
        let metadata = file.metadata().ok().filter(|m| m.is_file());
        let age = metadata
            .as_ref()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.elapsed().ok())
            .map(|d| d.as_secs());
        json!({"path": file, "present": metadata.is_some(), "age_seconds": age, "loaded": handler == "generated"})
    });
    let live = line
        .as_ref()
        .map(|line| crate::live::diagnose(&line.words, &line.cur, &line.scope));
    let report = json!({
        "version": crate::VERSION, "shell": shell, "binding": binding, "handler": handler,
        "command": name, "cache": cache, "live": live,
        "help_sandbox": if probe::sandbox_prefix().is_some() { "bubblewrap-configured" } else { "help-fallback" },
    });
    if a.has("--json") {
        println!("{report}");
    } else {
        summary(&report);
    }
    0
}

fn status(value: Option<String>, accepted: &[&str]) -> String {
    value
        .filter(|value| accepted.contains(&value.as_str()))
        .unwrap_or_else(|| "unknown".into())
}

fn summary(report: &Value) {
    let string = |v: &Value| v.as_str().unwrap_or_default().to_string();
    println!(
        "COMPLETE-ME {} · {}",
        string(&report["version"]),
        string(&report["shell"])
    );
    println!("Tab binding   {}", string(&report["binding"]));
    println!("Shell handler {}", string(&report["handler"]));
    println!("Help probe    {}", string(&report["help_sandbox"]));
    if let Some(cache) = report["cache"].as_object() {
        println!(
            "Cache         {} ({})",
            string(&cache["path"]),
            if cache["loaded"] == true {
                "loaded"
            } else if cache["present"] == true {
                "present"
            } else {
                "missing"
            }
        );
    }
    if let Some(live) = report["live"].as_object() {
        println!(
            "Live lookup   {} · {} candidates · {} ms",
            string(&live["status"]),
            live["matches"],
            live["elapsed_ms"]
        );
        for source in live["sources"].as_array().into_iter().flatten() {
            let scope = source["selectors"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(" ");
            println!(
                "  {} {} {} {}",
                string(&source["source"]),
                string(&source["operation"]),
                string(&source["status"]),
                scope
            );
        }
    }
}
