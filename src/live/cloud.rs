//! Read-only cloud inventory adapters. Only explicit resource slots query a service.
mod aws;
pub(super) use aws::{candidates as aws, object_slot as aws_object_slot};

use super::{
    arguments, field, metadata, query_with_timeout, record, remote_disabled, value_option, Rows,
};
use crate::util::{query_with_stdin, QueryStatus};
use serde_json::{json, Value};
use std::time::{Duration, Instant};

fn suggestions(names: &[&str], desc: &str) -> Rows {
    names
        .iter()
        .map(|name| ((*name).into(), desc.into()))
        .collect()
}

fn rows(value: &Value, base: &str, key: &str, columns: &[&str], prefix: &str) -> Rows {
    value[base]
        .as_array()
        .or_else(|| value.as_array())
        .into_iter()
        .flatten()
        .filter_map(|row| {
            let id = row[key].as_str().filter(|name| {
                !name.is_empty()
                    && !name.starts_with('-')
                    && !name.chars().any(|c| c.is_control() || c.is_whitespace())
            })?;
            Some((
                format!("{prefix}{id}"),
                metadata(
                    &columns
                        .iter()
                        .map(|column| field(row, column))
                        .collect::<Vec<_>>(),
                ),
            ))
        })
        .collect()
}

pub(super) fn vultr(words: &[String], current: &str) -> Rows {
    let value_flag = |flag: &str| matches!(flag, "--config" | "--output" | "-o" | "--region");
    let args = arguments(words, value_flag);
    if !args.options
        || args.switches.iter().any(|flag| {
            flag.starts_with("--config")
                || flag.starts_with("--api-")
                || flag.starts_with("--endpoint")
        })
    {
        return vec![];
    }
    let explicit = args.pending.map(|flag| (flag, 0)).or_else(|| {
        value_option(current, value_flag)
            .filter(|(_, offset)| *offset < current.len() || current.ends_with('='))
    });
    let (resource, list, key, fields, prefix): (_, &[&str], _, &[&str], _) = match explicit {
        Some(("--region", offset)) if args.positional.as_slice() == ["instance", "create"] => (
            "regions",
            &["regions", "list"],
            "id",
            &["city", "country", "continent"],
            &current[..offset],
        ),
        None if !current.starts_with('-') => match args.positional.as_slice() {
            [] => {
                return suggestions(
                    &["instance", "regions", "dns", "kubernetes"],
                    "Vultr resource",
                )
            }
            ["instance"] => {
                return suggestions(
                    &["list", "get", "start", "stop", "reboot", "delete", "create"],
                    "Vultr instance command",
                )
            }
            ["dns"] => return suggestions(&["domain"], "Vultr DNS resource"),
            ["dns", "domain"] => {
                return suggestions(&["list", "get", "delete"], "Vultr domain command")
            }
            ["regions"] => return suggestions(&["list"], "Vultr region command"),
            ["kubernetes"] => {
                return suggestions(&["list", "get", "delete"], "Vultr Kubernetes command")
            }
            ["instance", "get" | "start" | "stop" | "reboot" | "delete"] => (
                "instances",
                &["instance", "list", "--per-page", "50"],
                "id",
                &["label", "status", "region", "main_ip"],
                "",
            ),
            ["dns", "domain", "get" | "delete"] => (
                "domains",
                &["dns", "domain", "list"],
                "domain",
                &["date_created"],
                "",
            ),
            ["kubernetes", "get" | "delete"] => (
                "vke_clusters",
                &["kubernetes", "list"],
                "id",
                &["label", "region", "status"],
                "",
            ),
            _ => return vec![],
        },
        _ => return vec![],
    };
    let scope: Vec<_> = args
        .values
        .iter()
        .filter(|(flag, _)| *flag == "--config")
        .map(|(flag, value)| format!("{flag}={value}"))
        .collect();
    let mut command = list.to_vec();
    command.extend(["--output", "json"]);
    let output = query_with_timeout("vultr-cli", &scope, &command, Duration::from_millis(1250)).0;
    rows(
        &serde_json::from_str(&output).unwrap_or_default(),
        resource,
        key,
        fields,
        prefix,
    )
}

pub(super) fn hf(tool: &str, words: &[String], current: &str) -> Rows {
    let args = arguments(words, |flag| {
        matches!(flag, "--repo-type" | "--cache-dir" | "--revision")
    });
    if args.pending.is_some() || !args.options || current.starts_with('-') {
        return vec![];
    }
    let cache_id = match args.positional.as_slice() {
        [] => {
            return suggestions(
                &["cache", "download", "repo", "upload"],
                "Hugging Face CLI command",
            )
        }
        ["cache"] => {
            return suggestions(
                &["ls", "rm", "verify", "prune"],
                "Hugging Face cache command",
            )
        }
        ["cache", "rm" | "verify"] => true,
        ["download" | "upload"] => false,
        _ => return vec![],
    };
    let mut command = vec!["cache", "ls", "--format", "json"];
    let chosen_kind = args
        .values
        .iter()
        .rev()
        .find_map(|&(flag, value)| (flag == "--repo-type").then_some(value));
    if let Some(dir) = args
        .values
        .iter()
        .rev()
        .find_map(|&(flag, value)| (flag == "--cache-dir").then_some(value))
    {
        command.extend(["--cache-dir", dir]);
    }
    let text = query_with_timeout(tool, &[], &command, Duration::from_millis(600)).0;
    let value: Value = serde_json::from_str(&text).unwrap_or_default();
    value
        .as_array()
        .or_else(|| value["repos"].as_array())
        .into_iter()
        .flatten()
        .filter_map(|row| {
            let (kind, id) =
                if let Some((kind, id)) = row["id"].as_str().and_then(|id| id.split_once('/')) {
                    (kind, id)
                } else {
                    (row["repo_type"].as_str()?, row["repo_id"].as_str()?)
                };
            if !matches!(kind, "model" | "dataset" | "space" | "kernel")
                || id.is_empty()
                || !id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"/._-".contains(&b))
                || !cache_id && kind != chosen_kind.unwrap_or("model")
            {
                return None;
            }
            let value = if cache_id {
                format!("{kind}/{id}")
            } else {
                id.into()
            };
            Some((
                value,
                metadata(&[
                    kind.into(),
                    field(row, "size_on_disk"),
                    field(row, "last_modified"),
                ]),
            ))
        })
        .collect()
}

fn api(tool: &str) -> Option<Value> {
    if remote_disabled() {
        record(|| json!({"source": tool, "status": "remote-disabled"}));
        return None;
    }
    let (key_var, url) = match tool {
        "lambda" => (
            "COMPLETE_ME_LAMBDA_API_KEY",
            "https://cloud.lambda.com/api/v1/instances?page_size=50",
        ),
        "dreamhost" => (
            "COMPLETE_ME_DREAMHOST_API_KEY",
            "https://api.dreamhost.com/",
        ),
        _ => return None,
    };
    let secret = std::env::var(key_var).ok().filter(|key| {
        !key.is_empty()
            && key.len() <= 512
            && key
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
    });
    let Some(secret) = secret else {
        record(|| json!({"source":tool,"status":"credentials-unavailable"}));
        return None;
    };
    let mut config = format!("url = \"{url}\"\n");
    if tool == "lambda" {
        config.push_str(&format!(
            "header = \"Authorization: Bearer {secret}\"\nheader = \"Accept: application/json\"\n"
        ));
    } else {
        config.push_str(&format!("get\ndata-urlencode = \"key={secret}\"\ndata-urlencode = \"cmd=dns-list_records\"\ndata-urlencode = \"format=json\"\n"));
    }
    let args = [
        "curl",
        "--disable",
        "--config",
        "-",
        "--proto",
        "=https",
        "--silent",
        "--fail",
        "--connect-timeout",
        "1",
        "--max-time",
        "1.25",
    ]
    .map(str::to_owned);
    let start = Instant::now();
    let (output, status) =
        query_with_stdin(&args, Duration::from_millis(1400), config.into_bytes());
    let value: Option<Value> = serde_json::from_str(&output).ok();
    record(|| {
        json!({
            "source":tool, "operation":"list", "elapsed_ms":start.elapsed().as_millis() as u64,
            "status":match status {
                QueryStatus::Success if value.is_none() => "invalid-output",
                QueryStatus::Success => "success",
                QueryStatus::Missing => "tool-missing",
                QueryStatus::Failed(_) => "failed",
                QueryStatus::Timeout => "timeout",
            }
        })
    });
    value
}

fn api_rows(tool: &str) -> Rows {
    let Some(value) = api(tool) else {
        return vec![];
    };
    match tool {
        "lambda" => value["data"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|item| {
                let id = item["id"].as_str().filter(|id| {
                    !id.is_empty() && id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-')
                })?;
                Some((
                    id.into(),
                    metadata(&[
                        field(item, "name"),
                        field(item, "status"),
                        field(&item["region"], "name"),
                        field(item, "ip"),
                    ]),
                ))
            })
            .collect(),
        "dreamhost" if value["result"] == "success" => {
            // DNS targets and TXT values may include authentication material. Use names and types only.
            rows(&value, "data", "record", &["type"], "")
        }
        _ => vec![],
    }
}

pub(super) fn api_candidates(words: &[String]) -> Rows {
    match words {
        [] => suggestions(&["lambda", "dreamhost"], "Cloud inventory provider"),
        [tool] if tool == "lambda" => suggestions(&["instances"], "List Lambda Cloud instances"),
        [tool] if tool == "dreamhost" => {
            suggestions(&["records"], "List DreamHost DNS record names")
        }
        [tool, action]
            if matches!(
                (tool.as_str(), action.as_str()),
                ("lambda", "instances") | ("dreamhost", "records")
            ) =>
        {
            api_rows(tool)
        }
        _ => vec![],
    }
}

pub(crate) fn run(args: &[String]) -> i32 {
    let [tool, action, tail @ ..] = args else {
        eprintln!("usage: complete-me cloud lambda instances [ID] | dreamhost records [NAME]");
        return 2;
    };
    if !matches!(
        (tool.as_str(), action.as_str()),
        ("lambda", "instances") | ("dreamhost", "records")
    ) || tail.len() > 1
    {
        eprintln!("complete-me: unsupported cloud inventory command");
        return 2;
    }
    let rows = api_rows(tool);
    if rows.is_empty() {
        eprintln!("complete-me: no records or inventory lookup unavailable");
        return 1;
    }
    let mut found = false;
    for (id, description) in rows {
        if tail.first().is_none_or(|key| *key == id) {
            println!("{id}\t{}", crate::render::clean_desc(&description));
            found = true;
        }
    }
    i32::from(!found)
}
