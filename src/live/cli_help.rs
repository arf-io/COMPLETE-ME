//! Discover the installed container CLI's command path using its own help.
//! A child is probed only if the preceding help explicitly listed it.
use super::{arguments, Rows};
use crate::{model::Command, probe};

fn read(binary: &str, path: &[String]) -> Option<Command> {
    let mut argv = vec![binary.to_owned()];
    argv.extend_from_slice(path);
    let text = probe::help_text(&argv, 0.25);
    if text.is_empty() || !probe::looks_like_help(&text) {
        return None;
    }
    let name = path.last().map(String::as_str).unwrap_or_else(|| binary.rsplit('/').next().unwrap_or(binary));
    Some(crate::parse::parse_help(&text, name, "help"))
}

pub(super) fn candidates(tool: &str, words: &[String], current: &str) -> Rows {
    // Only known container frontends use this fallback. A value that already
    // looks like an operand cannot name a subcommand in installed help.
    if current.contains(['/', '\n', '\r']) || words.len() > 24 {
        return vec![];
    }
    if words.last().is_some_and(|word| !word.starts_with('-') && !probe::safe_name(word))
        && !current.starts_with('-')
    {
        return vec![];
    }
    let Some(binary) = crate::util::which(tool) else { return vec![] };
    let Some(binary) = binary.to_str() else { return vec![] };
    let rows = candidates_from(words, current, |path| read(binary, path));
    if rows.iter().any(|(value, _)| value.starts_with(current)) {
        super::record(|| serde_json::json!({"source": tool, "status": "installed-help"}));
    }
    rows
}

fn candidates_from(mut words: &[String], current: &str, mut read: impl FnMut(&[String]) -> Option<Command>) -> Rows {
    let mut path = vec![];
    let Some(mut model) = read(&path) else { return vec![] };
    while let Some(word) = words.first().filter(|word| probe::safe_name(word)) {
        let Some(child) = model.subcommands.get(word)
            .or_else(|| model.subcommands.values().find(|child| child.aliases.contains(word)))
        else { break };
        if path.len() == 4 {
            return vec![];
        }
        path.push(child.name.clone());
        let Some(next) = read(&path) else { return vec![] };
        if probe::echoes_parent(&next, &model) {
            return vec![];
        }
        model = next;
        words = &words[1..];
    }
    let option = |flag: &str| model.option(flag).or_else(|| model.options.iter().find(|o| {
        o.flags().iter().any(|name| super::same_flag(flag, name))
    }));
    let takes = |flag: &str| option(flag).is_some_and(|opt| opt.arg.is_some());
    let args = arguments(words, takes);
    let attached = crate::context::value_option(current, takes)
        .filter(|(_, offset)| *offset < current.len() || current.ends_with('='));
    if let Some((flag, offset)) = args.pending.map(|flag| (flag, 0)).or(attached) {
        return option(flag).into_iter().flat_map(|opt| &opt.values)
            .map(|(value, desc)| (format!("{}{value}", &current[..offset]), desc.clone())).collect();
    }
    if args.options && current.starts_with('-') {
        return model.options.iter()
            .flat_map(|opt| opt.flags().into_iter().map(|flag| (flag, opt.desc.clone())))
            .collect();
    }
    if !args.positional.is_empty() || !args.options {
        return vec![];
    }
    model.subcommands.values()
        .flat_map(|child| std::iter::once(&child.name).chain(&child.aliases)
            .filter(|name| probe::safe_name(name)).map(|name| (name.clone(), child.desc.clone())))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn help_probes_follow_declared_commands_only() {
        let mut seen = vec![];
        let mut complete = |words: &[&str], current| {
            let words: Vec<_> = words.iter().map(|word| (*word).into()).collect();
            candidates_from(&words, current, |path| {
                seen.push(path.join(" "));
                match path.join(" ").as_str() {
                    "" => Some(crate::parse::parse_help("Commands:\n  container   Manage containers\n", "docker", "test")),
                    "container" => Some(crate::parse::parse_help("Commands:\n  logs        Fetch logs\n", "container", "test")),
                    "container logs" => Some(crate::parse::parse_help("Options:\n  --tail string   Number of lines\n", "logs", "test")),
                    other => panic!("unexpected help probe for {other:?}"),
                }
            })
        };
        assert!(complete(&["container"], "lo").iter().any(|(v, _)| v == "logs"));
        assert!(complete(&["container", "logs"], "--tai").iter().any(|(v, _)| v == "--tail"));
        assert!(complete(&["container", "not-a-command", "logs"], "lo").is_empty());
        assert_eq!(seen.last().unwrap(), "container");
    }
}
