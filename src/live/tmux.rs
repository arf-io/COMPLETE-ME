use super::{arguments, global, metadata, query, value_option, Rows};
use crate::model::{Command, Opt};

fn tmux_man() -> String {
    let text = crate::probe::man_text_timeout("tmux", 0.35);
    let mut rows = vec![];
    let mut lines = text.lines().peekable();
    while let Some(line) = lines.next() {
        let Some(m) = crate::util::find(crate::re!(r"^ {5}([a-z][a-z-]*-[a-z-]+)(?: (.*))?$"), line) else {
            continue;
        };
        let name = crate::util::group(&m, 1);
        let mut usage = crate::util::group(&m, 2).to_string();
        // Manual pages wrap the synopsis before expanding on the command.
        while usage.matches('[').count() > usage.matches(']').count() {
            let Some(next) = lines.next_if(|l| l.starts_with("             ") || l.starts_with('\t')) else {
                break;
            };
            usage.push(' ');
            usage.push_str(next.trim());
        }
        let alias = lines
            .peek()
            .and_then(|line| line.trim().strip_prefix("(alias: "))
            .and_then(|alias| alias.strip_suffix(')'))
            .unwrap_or("");
        rows.push(format!("{name}|{alias}|{usage}"));
    }
    rows.join("\n")
}

fn tmux_model(scope: &[String], allow_man: bool) -> Command {
    // -N avoids starting a server on a missing socket.
    let text = query(
        "tmux",
        scope,
        &[
            "-N",
            "list-commands",
            "-F",
            "#{command_list_name}|#{command_list_alias}|#{command_list_usage}",
        ],
    );
    let (text, source) = if text.is_empty() && allow_man {
        (tmux_man(), "man")
    } else {
        (text, "list-commands")
    };
    let mut model = Command::with_source("tmux", source);
    model.takes_files = false;
    for (flag, arg) in [
        ('c', "shell-command"),
        ('f', "file"),
        ('L', "socket-name"),
        ('S', "socket-path"),
    ] {
        model.options.push(Opt {
            shorts: vec![flag.to_string()],
            arg: Some(arg.into()),
            kind: if matches!(flag, 'f' | 'S') {
                "file".into()
            } else {
                String::new()
            },
            ..Opt::default()
        });
    }
    for flag in "2CDlNuVv".chars() {
        model.options.push(Opt {
            shorts: vec![flag.to_string()],
            ..Opt::default()
        });
    }
    for row in text.lines() {
        let mut parts = row.splitn(3, '|');
        let (Some(name), Some(alias), Some(usage)) = (parts.next(), parts.next(), parts.next()) else {
            continue;
        };
        if !crate::probe::safe_name(name) {
            continue;
        }
        let mut sub = Command::with_source(name, source);
        sub.takes_files = matches!(name, "source-file" | "load-buffer" | "save-buffer");
        if crate::probe::safe_name(alias) {
            sub.aliases.push(alias.into());
        }
        for group in crate::util::find_all(crate::re!(r"\[-([A-Za-z0-9]+)(?: ([^\[\]]+))?\]"), usage) {
            let flags = crate::util::group(&group, 1);
            let placeholder = crate::util::group(&group, 2);
            for flag in flags.chars() {
                let arg = (!placeholder.is_empty()).then(|| placeholder.to_string());
                let kind = match placeholder {
                    "working-directory" | "start-directory" => "dir",
                    "path" => "file",
                    _ => "",
                };
                sub.options.push(Opt {
                    shorts: vec![flag.to_string()],
                    arg,
                    desc: placeholder.into(),
                    kind: kind.into(),
                    ..Opt::default()
                });
            }
        }
        model.subcommands.insert(name.into(), sub);
    }
    model
}

pub(crate) fn gather() -> Option<Command> {
    let model = tmux_model(&[], true);
    (!model.subcommands.is_empty()).then_some(model)
}

pub(super) fn candidates(words: &[String], current: &str) -> Rows {
    let Some((scope, words)) = global(words, &["-c", "-f", "-L", "-S"], &["-L", "-S"]) else {
        return vec![];
    };
    let words = words.iter().rposition(|w| w == ";").map_or(words, |i| &words[i + 1..]);
    if words.is_empty() && current.starts_with('-') {
        return vec![]; // global switches are covered by the ordinary shell completion
    }
    let model = tmux_model(&scope, words.is_empty() || current.starts_with('-'));
    let Some(command) = words.first() else {
        return model
            .subcommands
            .values()
            .flat_map(|sub| {
                std::iter::once((sub.name.clone(), "tmux command".into())).chain(
                    sub.aliases
                        .iter()
                        .map(|alias| (alias.clone(), format!("alias for {}", sub.name))),
                )
            })
            .collect();
    };
    let Some(sub) = model
        .subcommands
        .get(command)
        .or_else(|| model.subcommands.values().find(|sub| sub.aliases.contains(command)))
        .or_else(|| {
            let mut matches = model.subcommands.values().filter(|sub| sub.name.starts_with(command));
            let first = matches.next()?;
            matches.next().is_none().then_some(first)
        })
    else {
        return vec![];
    };
    let takes = |flag: &str| sub.option(flag).is_some_and(|o| o.arg.is_some());
    let args = arguments(&words[1..], takes);
    // A shell command after split-window/new-window can have its own switches.
    if !args.positional.is_empty() || !args.options {
        return vec![];
    }
    let slot = args
        .pending
        .map(|flag| (flag, 0))
        .or_else(|| value_option(current, takes).filter(|(_, offset)| *offset < current.len()));
    let Some((flag, offset)) = slot else {
        return if current.starts_with('-') {
            sub.options
                .iter()
                .flat_map(|opt| opt.flags().into_iter().map(|flag| (flag, opt.desc.clone())))
                .collect()
        } else {
            vec![]
        };
    };
    let flag = if flag.starts_with('-') {
        flag.to_string()
    } else {
        format!("-{flag}")
    };
    let Some(kind) = sub
        .option(&flag)
        .and_then(|opt| opt.arg.as_deref())
        .and_then(|arg| arg.rsplit('-').next())
    else {
        return vec![];
    };
    let value = &current[offset..];
    let kind = if kind == "session" && matches!(sub.name.as_str(), "attach-session" | "switch-client") {
        if (sub.name == "switch-client" && value.starts_with('%')) || value.contains('.') {
            "pane"
        } else if value.contains(':') {
            "window"
        } else {
            kind
        }
    } else {
        kind
    };
    let (cmd, extra, format) = match kind {
        "session" => (
            "list-sessions",
            "",
            "#{session_name}\t#{session_windows} windows\t#{?session_attached,attached,detached}\t#{session_id}",
        ),
        "window" => (
            "list-windows",
            "-a",
            "#{session_name}:#{window_index}\t#{window_name}\t#{window_panes} panes\t#{window_id}",
        ),
        "pane" => (
            "list-panes",
            "-a",
            "#{session_name}:#{window_index}.#{pane_index}\t#{window_name}\t#{pane_current_command}\t#{pane_id}",
        ),
        "client" => (
            "list-clients",
            "",
            "#{client_name}\t#{session_name}\t#{client_termname}\t",
        ),
        _ => return vec![],
    };
    let mut argv = vec!["-N", cmd];
    if !extra.is_empty() {
        argv.push(extra);
    }
    argv.extend(["-F", format]);
    let prefix = &current[..offset];
    query("tmux", &scope, &argv)
        .lines()
        .filter_map(|row| {
            let fields = row.split('\t').map(str::to_string).collect::<Vec<_>>();
            let target = fields.first()?.as_str();
            let id = fields.last()?.as_str();
            let target = if !id.is_empty() && value.starts_with(['$', '@', '%']) {
                id
            } else if kind == "window" && !value.is_empty() && !target.starts_with(value) {
                target
                    .split_once(':')
                    .filter(|(_, index)| index.starts_with(value))
                    .map_or(target, |(_, index)| index)
            } else {
                target
            };
            Some((format!("{prefix}{target}"), metadata(&fields[1..])))
        })
        .collect()
}
