//! Completion maps typed AWS actions to a fixed inventory call in the selected scope.
use crate::live::{arguments, field, metadata, query, query_with_timeout, value_option, Rows};
use serde_json::Value;
use std::io::Read;
use std::path::PathBuf;
use std::time::Duration;

const SERVICES: &[&str] = &[
    "configure",
    "ec2",
    "ecs",
    "eks",
    "lambda",
    "rds",
    "s3",
    "s3api",
];
const VALUE_FLAGS: &[&str] = &[
    "--profile",
    "--region",
    "--output",
    "--instance-ids",
    "--function-name",
    "--cluster",
    "--name",
    "--bucket",
    "--key",
    "--prefix",
    "--expected-bucket-owner",
    "--include",
    "--exclude",
    "--db-instance-identifier",
];
const BLOCKED: &[&str] = &[
    "--endpoint-url",
    "--no-verify-ssl",
    "--no-sign-request",
    "--ca-bundle",
    "--cli-input-json",
    "--cli-input-yaml",
    "--cli-auto-prompt",
];

struct Slot {
    service: &'static str,
    flag: &'static str,
    query: &'static [&'static str],
    items: &'static str,
    key: &'static str,
    description: &'static [&'static str],
}

const SLOTS: &[Slot] = &[
    Slot {
        service: "ec2",
        flag: "--instance-ids",
        query: &[
            "describe-instances",
            "--page-size",
            "25",
            "--max-items",
            "25",
        ],
        items: "Reservations",
        key: "InstanceId",
        description: &["State.Name", "InstanceType", "PrivateIpAddress"],
    },
    Slot {
        service: "eks",
        flag: "--name",
        query: &["list-clusters", "--page-size", "50", "--max-items", "50"],
        items: "clusters",
        key: "",
        description: &[],
    },
    Slot {
        service: "ecs",
        flag: "--cluster",
        query: &["list-clusters", "--page-size", "50", "--max-items", "50"],
        items: "clusterArns",
        key: "",
        description: &[],
    },
    Slot {
        service: "lambda",
        flag: "--function-name",
        query: &["list-functions", "--max-items", "50"],
        items: "Functions",
        key: "FunctionName",
        description: &["Runtime", "LastModified", "MemorySize"],
    },
    Slot {
        service: "rds",
        flag: "--db-instance-identifier",
        query: &[
            "describe-db-instances",
            "--page-size",
            "25",
            "--max-items",
            "25",
        ],
        items: "DBInstances",
        key: "DBInstanceIdentifier",
        description: &["Engine", "DBInstanceStatus", "DBInstanceClass"],
    },
    Slot {
        service: "s3api",
        flag: "--bucket",
        query: &["list-buckets", "--page-size", "50", "--max-items", "50"],
        items: "Buckets",
        key: "Name",
        description: &["BucketRegion", "CreationDate"],
    },
];

fn verbs(service: &str) -> &[&str] {
    match service {
        "configure" => &["list-profiles", "list", "get", "set", "sso"],
        "ec2" => &[
            "describe-instances",
            "start-instances",
            "stop-instances",
            "reboot-instances",
            "terminate-instances",
            "describe-instance-status",
        ],
        "eks" => &[
            "list-clusters",
            "describe-cluster",
            "update-kubeconfig",
            "delete-cluster",
        ],
        "ecs" => &[
            "list-clusters",
            "describe-clusters",
            "list-services",
            "list-tasks",
            "describe-services",
            "describe-tasks",
            "update-service",
        ],
        "lambda" => &[
            "list-functions",
            "get-function",
            "get-function-configuration",
            "invoke",
            "publish-version",
            "list-versions-by-function",
            "update-function-configuration",
        ],
        "rds" => &[
            "describe-db-instances",
            "start-db-instance",
            "stop-db-instance",
            "reboot-db-instance",
            "delete-db-instance",
        ],
        "s3" => &["ls", "cp", "sync", "mv", "rm"],
        "s3api" => &[
            "list-buckets",
            "head-bucket",
            "get-bucket-location",
            "list-objects-v2",
            "get-object",
            "head-object",
            "get-object-attributes",
            "put-object",
            "delete-object",
        ],
        _ => &[],
    }
}

fn scope_value(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value.as_bytes()[0].is_ascii_alphanumeric()
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.@+-".contains(&b))
}

fn profiles(prefix: &str) -> Rows {
    query(
        "aws",
        &[],
        &[
            "configure",
            "list-profiles",
            "--no-cli-pager",
            "--no-cli-auto-prompt",
        ],
    )
    .lines()
    .filter(|profile| scope_value(profile))
    .map(|name| (format!("{prefix}{name}"), "AWS CLI profile".into()))
    .collect()
}

fn custom_endpoint(config: &str, profile: &str) -> bool {
    let mut selected = false;
    for line in config.lines().map(str::trim) {
        if let Some(section) = line.strip_prefix('[').and_then(|line| line.split_once(']')) {
            let name = section.0.trim();
            selected = name == "DEFAULT"
                || (name == "default" && profile == "default")
                || name
                    .strip_prefix("profile ")
                    .is_some_and(|name| name.trim().trim_matches(['"', '\'']) == profile);
        } else if selected {
            if let Some((key, _)) = line.split_once(['=', ':']) {
                if matches!(
                    key.trim().to_ascii_lowercase().as_str(),
                    "endpoint_url" | "services"
                ) {
                    return true;
                }
            }
        }
    }
    false
}

fn standard_aws_destination(scope: &[String], service: &str) -> bool {
    let endpoint = if service == "s3api" {
        "S3".into()
    } else {
        service.to_ascii_uppercase()
    };
    if [
        "AWS_ENDPOINT_URL",
        &format!("AWS_ENDPOINT_URL_{endpoint}"),
        "AWS_DATA_PATH",
    ]
    .iter()
    .any(|key| std::env::var_os(key).is_some_and(|value| !value.is_empty()))
    {
        return false;
    }
    let config = std::env::var_os("AWS_CONFIG_FILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| crate::util::home().join(".aws/config"));
    if !config.is_absolute()
        || std::env::var_os("AWS_SHARED_CREDENTIALS_FILE")
            .is_some_and(|path| !PathBuf::from(path).is_absolute())
    {
        return false;
    }
    let file = match crate::util::open_regular(config) {
        Ok(file) => file,
        Err(e) => return e.kind() == std::io::ErrorKind::NotFound,
    };
    let mut config = String::new();
    if file
        .take(256 * 1024 + 1)
        .read_to_string(&mut config)
        .is_err()
        || config.len() > 256 * 1024
    {
        return false;
    }
    let profile = scope
        .iter()
        .rev()
        .find_map(|arg| arg.strip_prefix("--profile="))
        .map(str::to_owned)
        .or_else(|| {
            crate::live::effective_env("aws", "AWS_DEFAULT_PROFILE")
                .or_else(|| crate::live::effective_env("aws", "AWS_PROFILE"))
                .map(|name| name.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| "default".into());
    !custom_endpoint(&config, &profile)
}

fn read(scope: &[String], service: &str, args: &[&str]) -> Value {
    if !standard_aws_destination(scope, service) {
        crate::live::record(
            || serde_json::json!({"source":"aws", "status":"scope-blocked", "reason":"custom-endpoint-or-config"}),
        );
        return Value::Null;
    }
    let command: Vec<_> = std::iter::once(service)
        .chain(args.iter().copied())
        .chain([
            "--output",
            "json",
            "--no-cli-pager",
            "--no-cli-auto-prompt",
            "--cli-connect-timeout",
            "1",
            "--cli-read-timeout",
            "1",
        ])
        .collect();
    let output = query_with_timeout("aws", scope, &command, Duration::from_millis(1350)).0;
    serde_json::from_str(&output).unwrap_or_default()
}

fn entries<'a>(value: &'a Value, slot: &Slot) -> Box<dyn Iterator<Item = &'a Value> + 'a> {
    let list = value[slot.items].as_array().into_iter().flatten();
    if slot.service == "ec2" {
        return Box::new(list.flat_map(|item| item["Instances"].as_array().into_iter().flatten()));
    }
    Box::new(list)
}

fn property(row: &Value, key: &str) -> String {
    if let Some((parent, child)) = key.split_once('.') {
        field(&row[parent], child)
    } else {
        field(row, key)
    }
}

fn inventory(scope: &[String], slot: &Slot, prefix: &str) -> Rows {
    let value = read(scope, slot.service, slot.query);
    entries(&value, slot)
        .filter_map(|row| {
            let value = if slot.key.is_empty() {
                row.as_str()?.to_owned()
            } else {
                field(row, slot.key)
            };
            if value.is_empty()
                || !value
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b":/._-".contains(&b))
            {
                return None;
            }
            let mut fields = vec![];
            if slot.service == "ec2" {
                fields.push(
                    row["Tags"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .find(|item| item["Key"] == "Name")
                        .map(|item| field(item, "Value"))
                        .unwrap_or_default(),
                );
            }
            fields.extend(slot.description.iter().map(|key| property(row, key)));
            fields.push(format!("AWS {}", slot.service));
            Some((format!("{prefix}{value}"), metadata(&fields)))
        })
        .collect()
}

fn bucket_name(bucket: &str) -> bool {
    (3..=63).contains(&bucket.len())
        && bucket.split('.').all(|label| {
            label
                .as_bytes()
                .first()
                .is_some_and(u8::is_ascii_alphanumeric)
                && label
                    .as_bytes()
                    .last()
                    .is_some_and(u8::is_ascii_alphanumeric)
                && label
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        })
        && bucket.parse::<std::net::Ipv4Addr>().is_err()
}

fn valid_key(key: &str) -> bool {
    key.len() <= 1024
        && !key.chars().any(|c| {
            c.is_control() || matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
}

fn object_rows(scope: &[String], bucket: &str, key: &str, lead: &str, owner: Option<&str>) -> Rows {
    if !bucket_name(bucket)
        || !valid_key(key)
        || owner
            .is_some_and(|owner| owner.len() != 12 || !owner.bytes().all(|b| b.is_ascii_digit()))
    {
        return vec![];
    }
    // MaxKeys is an API field, not an AWS CLI v2 flag. Disable CLI pagination so
    // a page with only CommonPrefixes is as bounded as a page with objects.
    let mut request = serde_json::json!({
        "Bucket": bucket, "Prefix": key, "Delimiter": "/", "MaxKeys": 50,
    });
    if let Some(owner) = owner {
        request["ExpectedBucketOwner"] = owner.into();
    }
    let request = request.to_string();
    let value = read(
        scope,
        "s3api",
        &[
            "list-objects-v2",
            "--no-paginate",
            "--cli-input-json",
            &request,
        ],
    );
    let acceptable = |candidate: &&str| {
        candidate.starts_with(key)
            && valid_key(candidate)
            && !candidate.is_empty()
            && (!lead.is_empty() || !candidate.starts_with('-'))
            && (lead.starts_with("s3://")
                || !candidate.starts_with("file://") && !candidate.starts_with("fileb://"))
    };
    let prefixes = value["CommonPrefixes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|row| {
            let path = row["Prefix"].as_str().filter(acceptable)?;
            path.ends_with('/')
                .then(|| (format!("{lead}{path}"), "S3 prefix".into()))
        });
    let objects = value["Contents"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|row| {
            let path = row["Key"].as_str().filter(acceptable)?;
            let size = row["Size"]
                .as_u64()
                .map(|n| format!("{n} B"))
                .unwrap_or_default();
            Some((
                format!("{lead}{path}"),
                metadata(&["S3 object".into(), size, field(row, "LastModified")]),
            ))
        });
    prefixes.chain(objects).take(50).collect()
}

fn value_flag(flag: &str) -> bool {
    VALUE_FLAGS.contains(&flag)
}

fn option_slot<'a>(
    args: &crate::live::Arguments<'a>,
    current: &'a str,
) -> Option<(&'a str, usize)> {
    args.pending.map(|flag| (flag, 0)).or_else(|| {
        value_option(current, value_flag)
            .filter(|(_, offset)| *offset < current.len() || current.ends_with('='))
    })
}

fn s3_value(service: &str, action: &str, slot: Option<(&str, usize)>, current: &str) -> bool {
    match (service, action, slot) {
        ("s3", _, None) => verbs(service).contains(&action) && current.starts_with("s3://"),
        (
            "s3api",
            "get-object" | "head-object" | "get-object-attributes" | "put-object" | "delete-object",
            Some(("--key", _)),
        )
        | ("s3api", "list-objects-v2", Some(("--prefix", _))) => true,
        _ => false,
    }
}

pub(in crate::live) fn object_slot(words: &[String], current: &str) -> bool {
    let args = arguments(words, value_flag);
    matches!(args.positional.as_slice(), [service, action, ..]
        if s3_value(service, action, option_slot(&args, current), current))
}

fn blocked(value: &str) -> bool {
    BLOCKED.contains(&value.split('=').next().unwrap_or(value))
}

fn s3_objects(
    service: &str,
    args: &crate::live::Arguments<'_>,
    slot: Option<(&str, usize)>,
    mut scope: Vec<String>,
    current: &str,
    following: &[String],
) -> Rows {
    if following.iter().any(|word| blocked(word)) {
        return vec![];
    }
    let tail = arguments(following, value_flag);
    if tail.pending.is_some()
        || args.switches.iter().chain(&tail.switches).any(|flag| {
            !matches!(
                *flag,
                "--recursive"
                    | "--dryrun"
                    | "--no-progress"
                    | "--only-show-errors"
                    | "--summarize"
                    | "--human-readable"
            )
        })
    {
        return vec![];
    }
    for &(flag, value) in &tail.values {
        if matches!(flag, "--profile" | "--region") {
            if !scope_value(value) {
                return vec![];
            }
            scope.push(format!("{flag}={value}"));
        }
    }
    let value_for = |name| {
        tail.values
            .iter()
            .rev()
            .chain(args.values.iter().rev())
            .find_map(|(flag, value)| (*flag == name).then_some(*value))
    };
    let owner = value_for("--expected-bucket-owner");
    if service == "s3" {
        if let Some((bucket, key)) = current[5..].split_once('/') {
            return object_rows(&scope, bucket, key, &current[..6 + bucket.len()], owner);
        }
        let bucket_slot = SLOTS
            .iter()
            .find(|slot| slot.service == "s3api" && slot.flag == "--bucket");
        return bucket_slot
            .map(|slot| {
                inventory(&scope, slot, "s3://")
                    .into_iter()
                    .map(|(value, desc)| (format!("{value}/"), desc))
                    .collect()
            })
            .unwrap_or_default();
    }
    match (value_for("--bucket"), slot) {
        (Some(bucket), Some((_, offset))) => object_rows(
            &scope,
            bucket,
            &current[offset..],
            &current[..offset],
            owner,
        ),
        _ => vec![],
    }
}

pub(in crate::live) fn candidates(
    words: &[String],
    current: &str,
    right: &str,
    fish: bool,
) -> Rows {
    if words
        .iter()
        .map(String::as_str)
        .chain(std::iter::once(current))
        .any(blocked)
    {
        return vec![];
    }
    let args = arguments(words, value_flag);
    if !args.options {
        return vec![];
    }
    let slot = option_slot(&args, current);
    let mut scope = vec![];
    for &(flag, value) in &args.values {
        if matches!(flag, "--region" | "--profile") {
            if !scope_value(value) {
                return vec![];
            }
            scope.push(format!("{flag}={value}"));
        }
    }
    if let Some(("--profile", offset)) = slot {
        return profiles(&current[..offset]);
    }
    if let Some(("--region", offset)) = slot {
        let regions = read(&scope, "ec2", &["describe-regions", "--all-regions"]);
        return regions["Regions"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|row| {
                let region = row["RegionName"]
                    .as_str()
                    .filter(|region| scope_value(region))?;
                Some((
                    format!("{}{region}", &current[..offset]),
                    field(row, "OptInStatus"),
                ))
            })
            .collect();
    }
    let Some(service) = args.positional.first().copied() else {
        return SERVICES
            .iter()
            .map(|name| ((*name).into(), "AWS service".into()))
            .collect();
    };
    let Some(action) = args.positional.get(1).copied() else {
        return verbs(service)
            .iter()
            .map(|name| ((*name).into(), "AWS CLI operation".into()))
            .collect();
    };
    if !verbs(service).contains(&action) {
        return vec![];
    }
    if s3_value(service, action, slot, current) {
        let Some(following) = crate::cursor::following_words(right, fish) else {
            return vec![];
        };
        return s3_objects(service, &args, slot, scope, current, &following);
    }
    let (service, flag, prefix) = match slot {
        Some((flag, offset)) => (service, flag, &current[..offset]),
        None if service == "eks" && action == "update-kubeconfig" && current.starts_with("--") => {
            return vec![("--name=".into(), "Select the EKS cluster".into())];
        }
        None => return vec![],
    };
    let Some(slot) = SLOTS
        .iter()
        .find(|slot| slot.service == service && slot.flag == flag)
    else {
        return vec![];
    };
    inventory(&scope, slot, prefix)
}
