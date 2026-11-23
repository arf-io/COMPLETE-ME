//! dd operand completion from block metadata. No device is opened by COMPLETE-ME.
use super::{field, metadata, query, Rows};
use serde_json::Value;

const OPERANDS: &[(&str, &str)] = &[
    ("if=", "Input file or device"),
    ("of=", "Output file or device"),
    ("bs=", "Input and output block size"),
    ("ibs=", "Input block size"),
    ("obs=", "Output block size"),
    ("skip=", "Input offset"),
    ("seek=", "Output offset"),
    ("count=", "Number of input blocks"),
    ("iflag=", "Input flags"),
    ("oflag=", "Output flags"),
    ("conv=", "Conversions"),
    ("status=", "Progress reporting"),
];

fn number(row: &Value, key: &str) -> Option<u64> {
    row[key]
        .as_u64()
        .or_else(|| row[key].as_str()?.parse().ok())
}

fn name(row: &Value) -> Option<&str> {
    row["name"]
        .as_str()
        .filter(|name| name.starts_with("/dev/"))
}

fn read_only(row: &Value) -> bool {
    row["ro"] == true || number(row, "ro") == Some(1)
}

fn describe(row: &Value) -> String {
    let physical = number(row, "phy-sec");
    let logical = number(row, "log-sec");
    let geometry = match (logical, physical) {
        (Some(log), Some(phys)) => format!("{log}/{phys} B logical/physical"),
        (Some(log), None) => format!("{log} B logical"),
        _ => String::new(),
    };
    metadata(&[
        field(row, "type"),
        number(row, "start")
            .and_then(|n| n.checked_mul(512))
            .map_or(String::new(), |n| format!("starts {n} B")),
        geometry,
        number(row, "size").map_or(String::new(), |n| format!("{n} B total")),
        if read_only(row) {
            "read-only".into()
        } else {
            String::new()
        },
        if row["mountpoints"]
            .as_array()
            .is_some_and(|paths| paths.iter().any(Value::is_string))
        {
            "mounted".into()
        } else {
            String::new()
        },
    ])
}

fn device<'a>(rows: &'a [Value], rest: &[String], key: &str) -> Option<&'a Value> {
    let path = rest
        .iter()
        .rev()
        .find_map(|word| {
            word.split_once('=')
                .filter(|(operand, _)| *operand == key)
                .map(|(_, path)| path)
        })?
        .strip_prefix("/dev/")?;
    let path = format!("/dev/{path}");
    let canonical = path
        .starts_with("/dev/disk/")
        .then(|| std::fs::canonicalize(&path).ok())
        .flatten();
    rows.iter().find(|row| {
        name(row).is_some_and(|name| {
            name == path || canonical.as_deref() == Some(std::path::Path::new(name))
        })
    })
}

pub(super) fn candidates(rest: &[String], current: &str) -> Rows {
    let Some((key, value)) = current.split_once('=') else {
        return OPERANDS
            .iter()
            .filter(|(key, _)| !rest.iter().any(|word| word.starts_with(key)))
            .map(|(key, desc)| ((*key).into(), (*desc).into()))
            .collect();
    };
    match key {
        "if" | "of" if !value.starts_with("/dev") => return vec![],
        "if" | "of" | "skip" | "seek" | "bs" | "ibs" | "obs" => {}
        "status" => {
            return ["none", "noxfer", "progress"]
                .into_iter()
                .map(|value| (format!("status={value}"), "GNU dd reporting level".into()))
                .collect()
        }
        _ => return vec![],
    }
    let text = query(
        "lsblk",
        &[],
        &[
            "--json",
            "--bytes",
            "--paths",
            "--list",
            "--output",
            "NAME,TYPE,PKNAME,START,SIZE,LOG-SEC,PHY-SEC,RO,MOUNTPOINTS",
        ],
    );
    let inventory: Value = serde_json::from_str(&text).unwrap_or_default();
    let Some(rows) = inventory["blockdevices"].as_array() else {
        return vec![];
    };
    let input = device(rows, rest, "if");
    let output = device(rows, rest, "of");
    match key {
        "if" | "of" => rows
            .iter()
            .filter_map(|row| {
                let path = name(row)?;
                if key == "of" && read_only(row) {
                    return None;
                }
                Some((format!("{key}={path}"), describe(row)))
            })
            .collect(),
        "bs" | "ibs" | "obs" => [
            ("if", input.filter(|_| key != "obs")),
            ("of", output.filter(|_| key != "ibs")),
        ]
        .into_iter()
        .flat_map(|(side, row)| {
            row.into_iter().flat_map(move |row| {
                [("log-sec", "logical"), ("phy-sec", "physical")]
                    .into_iter()
                    .filter_map(move |(field, kind)| {
                        let size = number(row, field).filter(|&n| n > 0)?;
                        Some((
                            format!("{key}={size}"),
                            format!("{side} {kind} sector · {}", name(row)?),
                        ))
                    })
            })
        })
        .collect(),
        "skip" | "seek" => {
            let Some(selected) = (if key == "skip" { input } else { output }) else {
                return vec![];
            };
            let Some(selected_name) = name(selected) else {
                return vec![];
            };
            if selected["type"] == "part" {
                return vec![(
                    format!("{key}=0"),
                    format!("Start of {selected_name} · {}", describe(selected)),
                )];
            }
            rows.iter()
                .filter_map(|row| {
                    if row["type"] != "part" || row["pkname"].as_str()? != selected_name {
                        return None;
                    }
                    let bytes = number(row, "start")?.checked_mul(512)?;
                    Some((
                        format!("{key}={bytes}B"),
                        format!("{} · {}", name(row)?, describe(row)),
                    ))
                })
                .collect()
        }
        _ => vec![],
    }
}
