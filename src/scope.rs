//! Recognize literal connection selectors and wrappers without evaluating shell code.
use std::ffi::OsString;

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Scope {
    variables: Vec<(String, Option<String>)>,
    wrappers: Vec<String>,
    unsupported: Option<&'static str>,
}

fn variables(tool: &str) -> &'static [&'static str] {
    match tool {
        "docker" | "docker-compose" => &[
            "DOCKER_HOST",
            "DOCKER_CONTEXT",
            "DOCKER_CONFIG",
            "DOCKER_TLS",
            "DOCKER_TLS_VERIFY",
            "DOCKER_CERT_PATH",
            "COMPOSE_FILE",
            "COMPOSE_PROJECT_NAME",
            "COMPOSE_PROFILES",
            "COMPOSE_ENV_FILES",
            "COMPOSE_PATH_SEPARATOR",
        ],
        "podman" => &["CONTAINER_HOST", "CONTAINER_CONNECTION", "CONTAINER_SSHKEY"],
        "gh" => &["GH_HOST", "GH_REPO", "GH_CONFIG_DIR", "GIT_DIR", "GIT_WORK_TREE"],
        "git" => &["GIT_DIR", "GIT_WORK_TREE", "GIT_NAMESPACE"],
        "codex" => &["CODEX_HOME"],
        "claude" => &["CLAUDE_CONFIG_DIR"],
        "aws" => &["AWS_PROFILE", "AWS_DEFAULT_PROFILE", "AWS_REGION", "AWS_DEFAULT_REGION"],
        "hf" | "huggingface-cli" => &["HF_HOME", "HF_HUB_CACHE", "HUGGINGFACE_HUB_CACHE"],
        "kubectl" | "kube" | "complete-me" => &["KUBECONFIG"],
        "tmux" => &["TMUX", "TMUX_TMPDIR"],
        _ => &[],
    }
}

const EXECUTION_OVERRIDES: &[&str] = &[
    "PATH",
    "HOME",
    "SHELL",
    "LD_PRELOAD",
    "LD_LIBRARY_PATH",
    "DYLD_INSERT_LIBRARIES",
    "DYLD_LIBRARY_PATH",
    "GH_TOKEN",
    "GITHUB_TOKEN",
    "GH_ENTERPRISE_TOKEN",
    "GITHUB_ENTERPRISE_TOKEN",
];

impl Scope {
    pub(crate) fn for_tool(&self, tool: &str) -> Vec<(String, Option<String>)> {
        self.variables
            .iter()
            .filter(|(name, _)| variables(tool).contains(&name.as_str()))
            .cloned()
            .collect()
    }

    pub(crate) fn effective(&self, tool: &str, key: &str) -> Option<OsString> {
        if variables(tool).contains(&key) {
            if let Some((_, value)) = self.variables.iter().rev().find(|(name, _)| name == key) {
                return value.as_ref().map(OsString::from);
            }
        }
        std::env::var_os(key)
    }

    pub(crate) fn report(&self, tool: &str) -> serde_json::Value {
        let overrides: Vec<_> = self.for_tool(tool).into_iter().map(|(name, _)| name).collect();
        serde_json::json!({"wrappers": self.wrappers, "environment_selectors": overrides, "status": self.blocker(tool)})
    }

    pub(crate) fn blocker(&self, tool: &str) -> Option<&'static str> {
        self.unsupported.or_else(|| {
            self.variables.iter().find_map(|(name, value)| {
                if EXECUTION_OVERRIDES.contains(&name.as_str()) || name.starts_with("GIT_CONFIG")
                    || (tool == "aws" && name.starts_with("AWS_") && !variables(tool).contains(&name.as_str()))
                    || (tool == "vultr-cli" && name.starts_with("VULTR_"))
                    || (matches!(tool, "cm-cloud" | "complete-me") && matches!(name.as_str(), "COMPLETE_ME_LAMBDA_API_KEY" | "COMPLETE_ME_DREAMHOST_API_KEY")) {
                    Some("execution-environment")
                } else if variables(tool).contains(&name.as_str())
                    && value
                        .as_deref()
                        .is_some_and(|v| v.contains(['$', '`', '*', '?', '\n', '\r']) || v.starts_with('~'))
                {
                    Some("non-literal-selector")
                } else {
                    None
                }
            })
        })
    }
}

fn variable_name(s: &str) -> bool {
    let mut bytes = s.bytes();
    bytes.next().is_some_and(|b| b.is_ascii_alphabetic() || b == b'_')
        && bytes.all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// Return the command words and their execution context. Unsupported wrapper
/// options block live queries so they cannot silently target a different runtime.
pub(crate) fn normalize(mut words: Vec<String>) -> (Vec<String>, Scope) {
    let mut scope = Scope::default();
    let mut start = 0;
    while let Some(word) = words.get(start) {
        if let Some((name, value)) = word.split_once('=').filter(|(name, _)| variable_name(name)) {
            scope.variables.retain(|(old, _)| old != name);
            scope.variables.push((name.into(), Some(value.into())));
            start += 1;
            continue;
        }
        let wrapper = word.rsplit('/').next().unwrap_or(word);
        if !matches!(
            wrapper,
            "env" | "sudo" | "doas" | "command" | "exec" | "time" | "nice" | "nohup" | "builtin" | "noglob"
        ) {
            break;
        }
        scope.wrappers.push(wrapper.into());
        if matches!(wrapper, "sudo" | "doas" | "builtin") {
            scope.unsupported = Some("execution-identity");
        }
        start += 1;
        while let Some(flag) = words.get(start).filter(|arg| arg.starts_with('-') && *arg != "-") {
            start += 1;
            match (wrapper, flag.as_str()) {
                (_, "--") => break,
                ("sudo" | "doas", "-u" | "-g" | "--user" | "--group") if words.get(start).is_some() => start += 1,
                ("sudo" | "doas", option)
                    if ["-n", "-H", "-E", "--non-interactive"].contains(&option)
                        || option.starts_with("--user=")
                        || option.starts_with("--group=") => {}
                ("time", "-p") => {}
                ("nice", "-n" | "--adjustment")
                    if words.get(start).is_some_and(|value| value.parse::<i32>().is_ok()) =>
                {
                    start += 1
                }
                ("env", "-u" | "--unset") if words.get(start).is_some_and(|name| variable_name(name)) => {
                    let name = &words[start];
                    scope.variables.retain(|(old, _)| old != name);
                    scope.variables.push((name.clone(), None));
                    start += 1;
                }
                ("env", flag) if flag.strip_prefix("--unset=").is_some_and(variable_name) => {
                    let name = &flag["--unset=".len()..];
                    scope.variables.retain(|(old, _)| old != name);
                    scope.variables.push((name.into(), None));
                }
                _ => {
                    scope.unsupported.get_or_insert("wrapper-option");
                    break;
                }
            }
        }
    }
    (words.split_off(start), scope)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(words: &[&str]) -> (Vec<String>, Scope) {
        normalize(words.iter().map(|word| (*word).into()).collect())
    }

    #[test]
    fn scopes_are_literal_and_tool_specific() {
        let (words, scope) = parse(&[
            "env",
            "-u",
            "DOCKER_CONTEXT",
            "DOCKER_HOST=unix:///tmp/lab",
            "command",
            "--",
            "docker",
            "logs",
        ]);
        assert_eq!(words, ["docker", "logs"]);
        assert_eq!(
            scope.for_tool("docker"),
            [
                ("DOCKER_CONTEXT".into(), None),
                ("DOCKER_HOST".into(), Some("unix:///tmp/lab".into()))
            ]
        );
        assert!(scope.for_tool("git").is_empty());
        assert_eq!(scope.blocker("docker"), None);
        assert_eq!(
            parse(&["PATH=/tmp", "docker"]).1.blocker("docker"),
            Some("execution-environment")
        );
        assert_eq!(
            parse(&["KUBECONFIG=$HOME/config", "kubectl"]).1.blocker("kubectl"),
            Some("non-literal-selector")
        );
    }

    #[test]
    fn identity_and_unknown_wrapper_flags_block_lookup() {
        for (words, reason) in [
            (&["sudo", "docker"][..], "execution-identity"),
            (&["doas", "-u", "root", "docker"][..], "execution-identity"),
            (&["env", "-i", "docker"][..], "wrapper-option"),
            (&["command", "-p", "docker"][..], "wrapper-option"),
        ] {
            assert_eq!(parse(words).1.blocker("docker"), Some(reason));
        }
        assert_eq!(
            parse(&["nice", "-n", "10", "time", "-p", "git", "fetch"]).0,
            ["git", "fetch"]
        );
    }
}
