//! Local interface names and command grammar for capture and network tools.
use super::Rows;
use std::ffi::CStr;

const IP: &[&str] = &[
    "address", "link", "route", "rule", "neigh", "netns", "maddress", "monitor", "tunnel", "vrf",
];
const ROUTE: &[&str] = &["show", "get", "add", "replace", "change", "del", "flush"];
const IP_ACTIONS: &[&str] = &[
    "show", "list", "add", "change", "replace", "del", "flush", "set",
];

fn interfaces() -> Rows {
    let entries = unsafe { libc::if_nameindex() };
    if entries.is_null() {
        return vec![];
    }
    let mut rows = vec![];
    let mut index = entries;
    unsafe {
        while (*index).if_index != 0 && !(*index).if_name.is_null() {
            if let Ok(name) = CStr::from_ptr((*index).if_name).to_str() {
                rows.push((
                    name.into(),
                    format!("Local interface #{}", (*index).if_index),
                ));
            }
            index = index.add(1);
        }
        libc::if_freenameindex(entries);
    }
    rows
}

fn values(names: &[&str], description: &str) -> Rows {
    names
        .iter()
        .map(|name| ((*name).into(), description.into()))
        .collect()
}

pub(super) fn candidates(tool: &str, words: &[String], current: &str) -> Rows {
    let last = words.last().map(String::as_str);
    match tool {
        "tcpdump" | "tshark" | "wireshark" => {
            if matches!(last, Some("-i" | "--interface")) {
                let mut rows = interfaces();
                if cfg!(target_os = "linux") {
                    rows.push(("any".into(), "All local capture interfaces".into()));
                }
                return rows;
            }
            if last == Some("-Y") && tool != "tcpdump" {
                return values(
                    &[
                        "arp", "dns", "http", "icmp", "icmpv6", "ip", "ipv6", "tcp", "tls", "udp",
                    ],
                    "Display filter protocol",
                );
            }
            if last == Some("-f") && tool != "tcpdump" {
                return values(
                    &["arp", "icmp", "icmp6", "ip", "ip6", "tcp", "udp"],
                    "Capture filter protocol",
                );
            }
            let attached = if current.starts_with("--interface=") && tool != "tcpdump" {
                Some("--interface=")
            } else if current.strip_prefix("-i").is_some_and(|s| !s.is_empty()) {
                Some("-i")
            } else {
                None
            };
            if let Some(prefix) = attached {
                return interfaces()
                    .into_iter()
                    .map(|(name, desc)| (format!("{prefix}{name}"), desc))
                    .collect();
            }
            if !current.starts_with('-') {
                return vec![];
            }
            let flags = match tool {
                "tcpdump" => &["-D", "-i", "-nn", "-p", "-r", "-w", "-c", "-s", "-v"][..],
                "tshark" => &["-D", "-i", "-f", "-Y", "-r", "-w", "-c", "-T"][..],
                _ => &["-D", "-i", "-f", "-Y", "-r", "-w", "-k"][..],
            };
            values(flags, "Capture or display option")
        }
        "ip" => {
            let tail = words
                .iter()
                .skip_while(|word| {
                    matches!(
                        word.as_str(),
                        "-4" | "-6" | "-br" | "-j" | "-o" | "-s" | "-d" | "-r" | "-nocolor"
                    )
                })
                .map(String::as_str)
                .collect::<Vec<_>>();
            match tail.as_slice() {
                [] => values(IP, "ip object"),
                ["r" | "route", "get" | "show" | "list" | "add" | "replace" | "change" | "del" | "flush", ..]
                    if matches!(last, Some("dev" | "oif" | "iif")) =>
                {
                    interfaces()
                }
                ["r" | "route", _, ..] if last == Some("table") => {
                    values(&["main", "local", "default", "all"], "Route table")
                }
                ["a" | "addr" | "address" | "l" | "link" | "n" | "neigh", _, ..]
                    if last == Some("dev") =>
                {
                    interfaces()
                }
                ["l" | "link", "set" | "show"] => {
                    let mut rows = values(&["dev"], "Select an interface");
                    rows.extend(interfaces());
                    rows
                }
                ["r" | "route"] => values(ROUTE, "Route operation"),
                ["a" | "addr" | "address" | "l" | "link" | "n" | "neigh" | "rule" | "netns"] => {
                    values(IP_ACTIONS, "ip operation")
                }
                ["r" | "route", "show" | "list" | "flush", ..] => {
                    values(&["dev", "table", "scope", "proto", "type"], "Route filter")
                }
                _ => vec![],
            }
        }
        "route" => match last {
            Some("dev" | "-ifp") => interfaces(),
            None => values(
                &[
                    "-n", "add", "del", "delete", "change", "get", "show", "flush",
                ],
                "Route command or option",
            ),
            _ => vec![],
        },
        "nmap" if current.starts_with('-') && !matches!(last, Some("-oN" | "-oX" | "-oA")) => {
            values(
                &["--help", "--version", "--iflist", "-oN", "-oX", "-oA"],
                "Nmap information or report output option",
            )
        }
        _ => vec![],
    }
}
