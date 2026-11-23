//! Agent CLI grammar and local metadata. Completing an agent command never launches an agent.
use super::{effective_env, field, metadata, value_option, Rows};
use crate::util::{config_home, home, open_regular};
use serde_json::Value;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};

struct Agent {
    name: &'static str,
    commands: &'static [&'static str],
    values: &'static [&'static str],
    switches: &'static [&'static str],
}

const AGENTS: &[Agent] = &[
    Agent {
        name: "codex",
        commands: &[
            "agents",
            "exec",
            "review",
            "resume",
            "fork",
            "mcp",
            "plugin",
            "doctor",
            "features",
            "completion",
            "help",
        ],
        values: &[
            "--model",
            "-m",
            "--profile",
            "--sandbox",
            "-s",
            "--cd",
            "-C",
            "--ask-for-approval",
            "-a",
        ],
        switches: &["--search", "--version", "--help"],
    },
    Agent {
        name: "claude",
        commands: &["agents", "auth", "mcp", "plugin", "doctor", "help"],
        values: &[
            "--model",
            "--resume",
            "-r",
            "--agent",
            "--permission-mode",
            "--output-format",
            "--input-format",
            "--add-dir",
        ],
        switches: &["--continue", "--print", "--version", "--help"],
    },
    Agent {
        name: "gemini",
        commands: &["mcp", "extensions", "skills", "hooks"],
        values: &[
            "--model",
            "-m",
            "--resume",
            "-r",
            "--approval-mode",
            "--output-format",
            "--include-directories",
        ],
        switches: &["--list-sessions", "--sandbox", "--version", "--help"],
    },
    Agent {
        name: "agy",
        commands: &["agents", "models", "changelog", "mcp", "skills", "help"],
        values: &[
            "--model",
            "--mode",
            "--effort",
            "--agent",
            "--conversation",
            "--output-format",
            "--add-dir",
        ],
        switches: &["--continue", "--sandbox", "--version", "--help"],
    },
    Agent {
        name: "hermes",
        commands: &[
            "chat", "config", "model", "sessions", "skills", "mcp", "doctor", "status",
        ],
        values: &["--model", "-m", "--provider", "--resume", "-r", "--profile"],
        switches: &["--continue", "--version", "--help"],
    },
    Agent {
        name: "cline",
        commands: &["auth", "config", "history", "task", "help", "version"],
        values: &["--model", "--cwd"],
        switches: &["--help", "--version"],
    },
    Agent {
        name: "opencode",
        commands: &[
            "run", "models", "session", "agent", "auth", "mcp", "stats", "export", "import",
            "upgrade",
        ],
        values: &[
            "--model",
            "-m",
            "--session",
            "-s",
            "--agent",
            "--format",
            "--dir",
        ],
        switches: &["--continue", "--help", "--version"],
    },
    Agent {
        name: "aider",
        commands: &[],
        values: &[
            "--model",
            "--editor-model",
            "--weak-model",
            "--edit-format",
            "--chat-history-file",
            "--config",
        ],
        switches: &[
            "--list-models",
            "--dry-run",
            "--no-auto-commits",
            "--help",
            "--version",
        ],
    },
    Agent {
        name: "cursor-agent",
        commands: &["status", "login", "logout", "ls", "resume", "help"],
        values: &["--model", "--mode", "--resume", "--output-format"],
        switches: &["--print", "--version", "--help"],
    },
    Agent {
        name: "copilot",
        commands: &[],
        values: &["--model", "--resume", "--agent", "--add-dir"],
        switches: &["--continue", "--version", "--help"],
    },
    Agent {
        name: "droid",
        commands: &["exec", "help"],
        values: &["--model", "--session-id", "--output-format"],
        switches: &["--version", "--help"],
    },
    Agent {
        name: "qwen",
        commands: &["mcp", "extensions"],
        values: &[
            "--model",
            "-m",
            "--resume",
            "--approval-mode",
            "--output-format",
        ],
        switches: &["--version", "--help"],
    },
    Agent {
        name: "goose",
        commands: &["session", "run", "configure", "recipe", "info"],
        values: &["--name", "--provider", "--model", "--recipe"],
        switches: &["--version", "--help"],
    },
    Agent {
        name: "amp",
        commands: &["threads", "tools", "mcp", "help"],
        values: &["--mode"],
        switches: &["--version", "--help"],
    },
    Agent {
        name: "pi",
        commands: &[],
        values: &["--model", "--provider", "--session", "--mode"],
        switches: &["--resume", "--list-models", "--help", "--version"],
    },
    // The name is also used by unrelated products. Extend it using the local catalog.
    Agent {
        name: "fx",
        commands: &[],
        values: &[],
        switches: &["--help", "--version"],
    },
];

pub(crate) fn known(tool: &str) -> bool {
    AGENTS.iter().any(|agent| agent.name == tool)
}

fn read(path: &Path, limit: usize) -> Option<String> {
    let mut text = String::new();
    open_regular(path)
        .ok()?
        .take((limit + 1) as u64)
        .read_to_string(&mut text)
        .ok()?;
    (text.len() <= limit).then_some(text)
}

fn entry(value: &str) -> bool {
    !value.is_empty() && value.len() <= 256 && !value.chars().any(char::is_control)
}

fn mapped(value: &Value, prefix: &str) -> Rows {
    value
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(name, desc)| {
            if !entry(name) || !desc.is_string() {
                return None;
            }
            Some((format!("{prefix}{name}"), field(value, name)))
        })
        .collect()
}

fn uuid(id: &str) -> bool {
    id.len() == 36
        && id.bytes().enumerate().all(|(i, b)| {
            if matches!(i, 8 | 13 | 18 | 23) {
                b == b'-'
            } else {
                b.is_ascii_hexdigit()
            }
        })
}

fn settings_dir(tool: &str, var: &str, fallback: &str) -> PathBuf {
    effective_env(tool, var)
        .filter(|path| !path.is_empty())
        .map_or_else(|| home().join(fallback), PathBuf::from)
}

fn codex_sessions() -> Rows {
    let path = settings_dir("codex", "CODEX_HOME", ".codex").join("session_index.jsonl");
    let Ok(file) = open_regular(path) else {
        return vec![];
    };
    BufReader::new(file.take(512 * 1024))
        .lines()
        .take(4000)
        .map_while(Result::ok)
        .filter_map(|line| {
            let row: Value = serde_json::from_str(&line).ok()?;
            let id = row["id"].as_str().filter(|id| uuid(id))?;
            let date = row["updated_at"]
                .as_str()
                .and_then(|date| date.get(..10))
                .filter(|date| date.bytes().all(|b| b.is_ascii_digit() || b == b'-'))
                .unwrap_or_default();
            Some((
                id.into(),
                metadata(&["Local Codex session".into(), date.into()]),
            ))
        })
        .collect()
}

fn claude_sessions() -> Rows {
    let Ok(cwd) = std::env::current_dir() else {
        return vec![];
    };
    let cwd = cwd
        .to_string_lossy()
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
        .collect::<String>();
    let dir = settings_dir("claude", "CLAUDE_CONFIG_DIR", ".claude")
        .join("projects")
        .join(cwd);
    let Ok(entries) = std::fs::read_dir(dir) else {
        return vec![];
    };
    entries
        .take(4000)
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let id = name.strip_suffix(".jsonl").filter(|id| uuid(id))?;
            entry
                .file_type()
                .ok()?
                .is_file()
                .then(|| (id.into(), "Local Claude project session".into()))
        })
        .collect()
}

fn value_rows(tool: &str, flag: &str, settings: &Value, prefix: &str) -> Rows {
    let model = matches!(
        flag,
        "--model" | "-m" | "m" | "--editor-model" | "--weak-model"
    );
    let mut rows = mapped(&settings["values"][flag], prefix);
    let fixed: &[&str] = match (tool, flag) {
        ("codex", "--sandbox" | "-s" | "s") => &["read-only", "workspace-write"],
        ("codex", "--ask-for-approval" | "-a" | "a") => &["untrusted", "on-request"],
        ("claude", "--permission-mode") => &["default", "acceptEdits", "plan", "dontAsk"],
        ("claude" | "agy", "--output-format") => &["text", "json", "stream-json"],
        ("gemini" | "qwen", "--output-format") => &["text", "json", "stream-json"],
        ("gemini" | "qwen", "--approval-mode") => &["default", "auto_edit", "plan"],
        ("agy", "--mode") => &["plan", "accept-edits"],
        ("agy", "--effort") => &["low", "medium", "high", "xhigh", "max"],
        ("opencode", "--format") => &["default", "json"],
        ("pi", "--mode") => &["text", "json", "rpc"],
        ("aider", "--edit-format") => &["whole", "diff", "udiff"],
        _ => &[],
    };
    rows.extend(
        fixed
            .iter()
            .map(|value| (format!("{prefix}{value}"), "Agent option value".into())),
    );
    if model {
        rows.extend(mapped(&settings["models"], prefix));
        match tool {
            "claude" => rows.extend(
                ["sonnet", "opus", "haiku"]
                    .into_iter()
                    .map(|model| (format!("{prefix}{model}"), "Claude model alias".into())),
            ),
            "gemini" => rows.push((
                format!("{prefix}auto"),
                "Gemini automatic model selection".into(),
            )),
            _ => {}
        }
    }
    if matches!(
        flag,
        "--resume" | "-r" | "r" | "--session" | "-s" | "s" | "--conversation" | "--session-id"
    ) {
        rows.extend(mapped(&settings["sessions"], prefix));
        if tool == "claude" {
            rows.extend(
                claude_sessions()
                    .into_iter()
                    .map(|(value, desc)| (format!("{prefix}{value}"), desc)),
            );
        }
        if tool == "gemini" {
            rows.push((
                format!("{prefix}latest"),
                "Most recent project session".into(),
            ));
        }
    }
    rows
}

fn settings(tool: &str) -> Value {
    read(&config_home().join("complete-me/agents.json"), 128 * 1024)
        .and_then(|data| serde_json::from_str::<Value>(&data).ok())
        .and_then(|mut catalog| catalog.get_mut(tool).map(Value::take))
        .unwrap_or_default()
}

pub(crate) fn model(tool: &str) -> Option<crate::model::Command> {
    use crate::model::{Command, Opt};
    let custom = settings(tool);
    let spec = AGENTS.iter().find(|agent| agent.name == tool);
    if spec.is_none() && !custom.is_object() {
        return None;
    }
    let mut command = Command::with_source(tool, "local agent completion catalog");
    for (value, desc) in candidates(tool, &[], "") {
        if crate::probe::safe_name(&value) {
            let mut sub = Command::new(&value);
            sub.desc = desc;
            command.subcommands.insert(value, sub);
        }
    }
    for (flag, desc) in candidates(tool, &[], "-") {
        let mut opt = if let Some(long) = flag.strip_prefix("--") {
            if !crate::probe::safe_name(long) {
                continue;
            }
            Opt {
                longs: vec![long.into()],
                ..Opt::default()
            }
        } else if let Some(short) = flag.strip_prefix('-').filter(|short| short.len() == 1) {
            Opt {
                shorts: vec![short.into()],
                ..Opt::default()
            }
        } else {
            continue;
        };
        opt.desc = desc;
        command.options.push(opt);
    }
    Some(command)
}

pub(super) fn candidates(tool: &str, words: &[String], current: &str) -> Rows {
    if words.iter().any(|word| word == "--") {
        return vec![];
    }
    let settings = settings(tool);
    let settings = &settings;
    let agent = AGENTS.iter().find(|agent| agent.name == tool);
    if agent.is_none() && !settings.is_object() {
        return vec![];
    }
    let flags = |flag: &str| {
        agent.is_some_and(|agent| agent.values.contains(&flag))
            || flag.starts_with("--") && settings["values"][flag].is_object()
    };
    if let Some((flag, offset)) = value_option(current, flags)
        .filter(|&(_, offset)| offset < current.len() || current.ends_with('='))
    {
        return value_rows(tool, flag, settings, &current[..offset]);
    }
    if let Some(flag) = words.last().filter(|word| flags(word)) {
        return value_rows(tool, flag, settings, "");
    }
    let parsed = super::arguments(words, flags);
    if current.starts_with('-') {
        let mut rows: Rows = agent
            .into_iter()
            .flat_map(|agent| agent.values.iter().chain(agent.switches))
            .map(|flag| ((*flag).into(), "Agent CLI option".into()))
            .collect();
        if let Some(values) = settings["values"].as_object() {
            rows.extend(
                values
                    .keys()
                    .filter(|name| {
                        name.starts_with("--") && !name.contains(char::is_whitespace) && entry(name)
                    })
                    .map(|name| (name.clone(), "Configured option".into())),
            );
        }
        return rows;
    }
    match (tool, parsed.positional.as_slice()) {
        ("codex", ["resume" | "fork" | "archive" | "unarchive" | "queue"] | ["exec", "resume"]) => {
            let mut rows = mapped(&settings["sessions"], "");
            rows.extend(codex_sessions());
            rows
        }
        ("codex", ["exec"]) => vec![("resume".into(), "Resume a headless session".into())],
        ("codex" | "claude" | "gemini" | "agy" | "opencode" | "hermes", ["mcp"]) => {
            ["list", "add", "remove"]
                .into_iter()
                .map(|cmd| (cmd.into(), "MCP command".into()))
                .collect()
        }
        ("opencode", ["session"]) => vec![("list".into(), "List OpenCode sessions".into())],
        (_, []) => {
            let mut rows: Rows = agent
                .into_iter()
                .flat_map(|agent| agent.commands)
                .map(|cmd| ((*cmd).into(), "Agent CLI command".into()))
                .collect();
            rows.extend(mapped(&settings["commands"], ""));
            rows
        }
        _ => vec![],
    }
}
