//! Ask real shells what they would complete, with complete-me loaded in an isolated environment.
use complete_me::{parse, render, util::fq};
use std::path::{Path, PathBuf};
use std::process::Command;

fn have(cmd: &str) -> bool {
    complete_me::util::which(cmd).is_some()
}

struct Env {
    tmp: PathBuf,
    path: String,
}

impl Env {
    fn new(tag: &str) -> Env {
        let tmp = std::env::temp_dir().join(format!("cm-e2e-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let bin = tmp.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_complete-me"), bin.join("complete-me")).unwrap();
        let gen = tmp.join("cache/complete-me");
        for d in ["fish", "bash", "zsh"] {
            std::fs::create_dir_all(gen.join(d)).unwrap();
        }
        for (fx, as_name) in [("zerotier-cli", "zerotier-cli"), ("cargo", "cargo-like")] {
            let mut c = parse::parse_help(&std::fs::read_to_string(format!("tests/fixtures/{fx}.help")).unwrap(), as_name, "help");
            c.name = as_name.into();
            std::fs::write(gen.join(format!("fish/{as_name}.fish")), render::fish(&c).unwrap()).unwrap();
            std::fs::write(gen.join(format!("bash/{as_name}")), render::bash(&c).unwrap()).unwrap();
            std::fs::write(gen.join(format!("zsh/{as_name}.zsh")), render::zsh(&c).unwrap()).unwrap();
        }
        std::fs::create_dir_all(tmp.join("data/fish")).unwrap();
        std::fs::write(tmp.join("data/fish/fish_history"), "- cmd: zerotier-cli join 8056c2e21c000001\n  when: 1790000000\n").unwrap();
        let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());
        let env = Env { tmp, path };
        // Fish only autoloads generated completions for an executable on PATH.
        env.executable("zerotier-cli", "exit 1");
        env.executable("cargo-like", "exit 1");
        env
    }
    fn cmd(&self, prog: &str) -> Command {
        let mut c = Command::new(prog);
        c.current_dir(&self.tmp)
            .env("PATH", &self.path)
            .env("XDG_CACHE_HOME", self.tmp.join("cache"))
            .env("XDG_CONFIG_HOME", self.tmp.join("config"))
            .env("XDG_DATA_HOME", self.tmp.join("data"))
            .env("AWS_CONFIG_FILE", self.tmp.join("aws-config"))
            .env("AWS_SHARED_CREDENTIALS_FILE", self.tmp.join("aws-credentials"))
            .env_remove("AWS_ENDPOINT_URL")
            .env_remove("AWS_ENDPOINT_URL_S3")
            .env_remove("AWS_DATA_PATH")
            .env("complete_me_force", "1");
        c
    }
    fn fish_with(&self, line: &str, clip: Option<&str>) -> Vec<(String, String)> {
        self.fish_with_env(line, clip, &[])
    }
    fn fish_with_env(&self, line: &str, clip: Option<&str>, env: &[(&str, &str)]) -> Vec<(String, String)> {
        let rt = Path::new(env!("CARGO_MANIFEST_DIR")).join("shell/fish/complete_me.fish");
        let script = format!("set -g fish_history fish; source {}; function cargo-like; end; complete -C {}", rt.display(), fq(line));
        let mut c = self.cmd("fish");
        c.args(["--no-config", "-c", &script]).envs(env.iter().copied());
        if let Some(t) = clip {
            c.env("COMPLETE_ME_CLIPBOARD", t);
        }
        let out = c.output().unwrap();
        assert_eq!(String::from_utf8_lossy(&out.stderr), "", "fish stderr for {line:?}");
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(|l| {
                let mut it = l.splitn(2, '\t');
                (it.next().unwrap_or("").to_string(), it.next().unwrap_or("").to_string())
            })
            .collect()
    }
    fn fish(&self, line: &str) -> Vec<String> {
        self.fish_with(line, None).into_iter().map(|(v, _)| v).collect()
    }
    fn clip(&self, text: &str, line: &str) -> Vec<String> {
        self.fish_with(line, Some(text)).into_iter().filter(|(_, d)| d.contains("clipboard")).map(|(v, _)| v).collect()
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.tmp);
    }
}

#[test]
fn fish_end_to_end() {
    if !have("fish") {
        eprintln!("fish not installed; skipping");
        return;
    }
    let e = Env::new("fish");
    // subcommands, then that subcommand's arguments from history
    assert!(e.fish("zerotier-cli list").contains(&"listnetworks".to_string()));
    assert!(e.fish("zerotier-cli join ").contains(&"8056c2e21c000001".to_string()));
    // enum values and flags
    assert_eq!(e.fish("cargo-like --color "), ["always", "auto", "never"]);
    assert!(e.fish("cargo-like --co").contains(&"--color".to_string()));
    // find -exec terminators
    assert_eq!(e.fish("find . -exec rm ")[0], "{}");
    assert_eq!(e.fish("find . -exec rm {} ")[..2], [";", "+"]);
    // the regex slot is only the pattern position
    assert!(e.fish("grep -E ab").contains(&"ab[0-9]".to_string()));
    assert!(!e.fish("grep -E pat ab").contains(&"ab[0-9]".to_string()));
    // clipboard: offered only when it fits
    assert_eq!(e.clip("https://github.com/x/y", "git clone "), ["https://github.com/x/y"]);
    assert_eq!(e.clip("git@host:me/repo.git", "git clone "), ["git@host:me/repo.git"]);
    assert!(e.clip("https://example.com/f.tgz", "git clone ").is_empty());
    assert_eq!(e.clip("https://example.com/f.tgz", "curl -O "), ["https://example.com/f.tgz"]);
    assert!(e.clip("hunter2 is just some copied prose", "curl ").is_empty());
    assert!(e.clip("https://a.example\nhttps://b.example", "curl ").is_empty());
    assert!(e.clip(&format!("{}/nope", e.tmp.display()), "less ").is_empty());
    // ssh -oKey=Value
    if !complete_me::probe::man_text("ssh_config").is_empty() {
        let mut got = e.fish("ssh -oPasswordAuthentication=");
        got.sort();
        assert_eq!(got, ["-oPasswordAuthentication=no", "-oPasswordAuthentication=yes"]);
    }
}

#[test]
fn bash_end_to_end() {
    if !have("bash") {
        return;
    }
    let e = Env::new("bash");
    let f = e.tmp.join("cache/complete-me/bash");
    let run = |file: &str, func: &str, words: &str, cword: usize| -> String {
        let script = format!("source {}/{file}; COMP_WORDS=({words}); COMP_CWORD={cword}; {func}; echo \"${{COMPREPLY[*]}}\"", f.display());
        String::from_utf8_lossy(&e.cmd("bash").args(["--norc", "-c", &script]).output().unwrap().stdout).trim().to_string()
    };
    assert_eq!(run("zerotier-cli", "_complete_me_zerotier_cli", "zerotier-cli li", 1), "listpeers listnetworks");
    assert_eq!(run("zerotier-cli", "_complete_me_zerotier_cli", "zerotier-cli -", 1), "-h -v -j -D -p -T");
    assert_eq!(run("cargo-like", "_complete_me_cargo_like", "cargo-like --color ''", 2), "auto always never");
    assert_eq!(run("cargo-like", "_complete_me_cargo_like", "cargo-like b", 1), "build bench b");
    let rt = Path::new(env!("CARGO_MANIFEST_DIR")).join("shell/bash/complete_me.bash");
    assert!(e.cmd("bash").args(["-n", rt.to_str().unwrap()]).status().unwrap().success());
}

#[test]
fn zsh_files_parse() {
    if !have("zsh") {
        eprintln!("zsh not installed; skipping");
        return;
    }
    let e = Env::new("zsh");
    for f in ["zerotier-cli.zsh", "cargo-like.zsh"] {
        let p = e.tmp.join("cache/complete-me/zsh").join(f);
        let out = e.cmd("zsh").args(["-n", p.to_str().unwrap()]).output().unwrap();
        assert!(out.status.success(), "{f}: {}", String::from_utf8_lossy(&out.stderr));
    }
    let rt = Path::new(env!("CARGO_MANIFEST_DIR")).join("shell/zsh/complete_me.zsh");
    assert!(e.cmd("zsh").args(["-n", rt.to_str().unwrap()]).status().unwrap().success());
}

// ---------------------------------------------------------------- bash and zsh: the extras

fn harness(name: &str) -> String {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/harness").join(name).display().to_string()
}

impl Env {
    fn with_history(&self, lines: &[&str]) -> PathBuf {
        let h = self.tmp.join("histfile");
        std::fs::write(&h, lines.iter().map(|l| format!("{l}\n")).collect::<String>()).unwrap();
        let zork = self.tmp.join("bin/zork");
        std::fs::write(&zork, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&zork, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        h
    }
    /// What bash offers, with or without bash-completion underneath.
    fn bash(&self, line: &str, words: &[&str], clip: Option<&str>, bash_completion: bool) -> Vec<String> {
        let rt = Path::new(env!("CARGO_MANIFEST_DIR")).join("shell/bash/complete_me.bash");
        let mut c = self.cmd("bash");
        c.args(["--norc", "--noprofile", &harness("bash_capture.bash"), rt.to_str().unwrap(), line]).args(words);
        c.env("HISTFILE", self.tmp.join("histfile"));
        if !bash_completion {
            c.env("CM_NO_BASH_COMPLETION", "1");
        }
        c.env("COMPLETE_ME_CLIPBOARD", clip.unwrap_or(""));
        let out = c.output().unwrap();
        String::from_utf8_lossy(&out.stdout).lines().map(String::from).collect()
    }
    /// What zsh offers: (group, match) — group is cm-<extra> or "native".
    fn zsh(&self, line: &str, clip: Option<&str>) -> Vec<(String, String)> {
        let rt = Path::new(env!("CARGO_MANIFEST_DIR")).join("shell/zsh/complete_me.zsh");
        let mut c = self.cmd("zsh");
        c.args(["-f", &harness("zsh_capture.zsh"), rt.to_str().unwrap(), line]);
        c.env("HISTFILE", self.tmp.join("histfile")).env("COMPLETE_ME_CLIPBOARD", clip.unwrap_or(""));
        let out = c.output().unwrap();
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter_map(|l| l.split_once('\t').map(|(g, w)| (g.to_string(), w.to_string())))
            .collect()
    }
}

fn has_bash_completion() -> bool {
    ["/usr/share/bash-completion/bash_completion", "/usr/local/share/bash-completion/bash_completion",
        "/opt/homebrew/share/bash-completion/bash_completion", "/home/linuxbrew/.linuxbrew/share/bash-completion/bash_completion"]
        .iter()
        .any(|p| Path::new(p).exists())
}

#[test]
fn bash_extras() {
    if !have("bash") {
        return;
    }
    let e = Env::new("bash-extras");
    e.with_history(&["ssh -p 2222 beta.example", "zork frobnicate", "zork /tmp"]);
    let modes: &[bool] = if has_bash_completion() { &[false, true] } else { &[false] };
    for &bc in modes {
        let ctx = if bc { "with bash-completion" } else { "plain bash" };
        // regex building blocks, quoted, and nothing else in the pattern slot
        // unquoted: whole quoted patterns
        let got = e.bash("grep -E ", &[], None, bc);
        assert!(got.iter().all(|w| w.starts_with('\'') && w.ends_with('\'')), "{ctx}: {got:?}");
        // inside an open quote readline replaces only what follows the quote: continue it, then close it
        let got = e.bash("grep -E '\\d", &[], None, bc);
        assert!(got.contains(&"\\d+'".to_string()), "{ctx}: {got:?}");
        assert!(got.iter().all(|w| w.starts_with("\\d")), "{ctx}: {got:?}");
        // find values, -exec terminators
        assert!(e.bash("find . -type ", &[], None, bc).contains(&"f".to_string()), "{ctx}");
        let got = e.bash("find . -exec rm {} ", &[], None, bc);
        assert!(got.contains(&r"\;".to_string()) && got.contains(&"+".to_string()), "{ctx}: {got:?}");
        // the clipboard fits the slot or stays out
        let url = "git@github.com:arf-io/COMPLETE-ME.git";
        assert!(e.bash("git clone ", &[], Some(url), bc).contains(&url.to_string()), "{ctx}");
        assert!(!e.bash("git clone ", &[], Some("hunter2"), bc).contains(&"hunter2".to_string()), "{ctx}");
        // history: arguments used before, most recent first, for a command nothing else knows
        let got = e.bash("zork ", &[], None, bc);
        assert_eq!(got[..2], ["/tmp/", "frobnicate"], "{ctx}: {got:?}");
        // ssh -oKey=Value: bash splits at '=', and readline replaces only what follows it
        if !complete_me::probe::man_text("ssh_config").is_empty() {
            let mut got = e.bash("ssh -oPasswordAuthentication=", &["ssh", "-oPasswordAuthentication", "="], None, bc);
            got.sort();
            assert_eq!(got, ["no", "yes"], "{ctx}");
        }
    }
    let rt = Path::new(env!("CARGO_MANIFEST_DIR")).join("shell/bash/complete_me.bash");
    let script = r#"
        COMPLETE_ME_NO_PREGEN=1 COMPLETE_ME_NO_KEYS=1 source "$1"
        _native_find() { COMPREPLY=(); return 124; }
        complete -F _native_find find
        _cm_wrap find
        COMP_LINE='find . -exec rm {} '
        COMP_POINT=${#COMP_LINE}
        COMP_WORDS=(find . -exec rm '{}' '')
        COMP_CWORD=5
        _cm_aug find '' '{}'
        printf '%s\n' "${COMPREPLY[@]}"
    "#;
    let result = e.cmd("bash").args(["--norc", "-c", script, "test", rt.to_str().unwrap()]).output().unwrap();
    let output = String::from_utf8(result.stdout).unwrap();
    assert!(output.lines().any(|row| row == r"\;") && output.lines().any(|row| row == "+"), "{output:?}");
    // native completions stay intact underneath
    if has_bash_completion() {
        assert!(e.bash("ls --colo", &[], None, true).iter().any(|w| w.starts_with("--color")));
    }

    // Older bash-completion versions return filenames from the generic handler.
    let generic = r#"
        history -r "$HISTFILE"
        _minimal() { COMPREPLY=(config data); }
        complete -F _minimal zork
        COMPLETE_ME_NO_PREGEN=1 COMPLETE_ME_NO_KEYS=1 source "$1"
        _cm_wrap zork
        COMP_LINE='zork '
        COMP_POINT=${#COMP_LINE}
        COMP_WORDS=(zork '')
        COMP_CWORD=1
        _cm_aug zork '' zork
        printf '%s\n' "${COMPREPLY[@]}"
    "#;
    let got = e.cmd("bash").env("HISTFILE", e.tmp.join("histfile"))
        .args(["--norc", "-c", generic, "test", rt.to_str().unwrap()]).output().unwrap();
    assert!(got.status.success(), "{}", String::from_utf8_lossy(&got.stderr));
    assert_eq!(String::from_utf8_lossy(&got.stdout).lines().collect::<Vec<_>>(), ["/tmp/", "frobnicate", "config", "data"]);
}

#[test]
fn zsh_extras() {
    if !have("zsh") {
        return;
    }
    let e = Env::new("zsh-extras");
    e.with_history(&["ssh -p 2222 beta.example", "zork frobnicate"]);
    let words = |v: Vec<(String, String)>, g: &str| v.into_iter().filter(|(gg, _)| gg == g).map(|(_, w)| w).collect::<Vec<_>>();
    // regex: only complete-me's group, bare inside the open quote
    let got = e.zsh("grep -E '\\d", None);
    assert!(got.iter().all(|(g, _)| g == "cm-regex"), "{got:?}");
    assert!(words(got, "cm-regex").contains(&"\\d+".to_string()));
    assert!(words(e.zsh("find . -type ", None), "cm-find").contains(&"f".to_string()));
    // history is its own group, next to zsh's own host completion
    let got = e.zsh("ssh ", None);
    assert_eq!(words(got.clone(), "cm-hist"), ["beta.example"], "{got:?}");
    assert!(got.iter().any(|(g, _)| g == "native"), "zsh's own completion should still run: {got:?}");
    assert_eq!(words(e.zsh("zork ", None), "cm-hist"), ["frobnicate"]);
    let url = "https://github.com/arf-io/COMPLETE-ME";
    assert_eq!(words(e.zsh("git clone ", Some(url)), "cm-clip"), [url]);
    if !complete_me::probe::man_text("ssh_config").is_empty() {
        let mut got = words(e.zsh("ssh -oPasswordAuthentication=", None), "cm-ssh");
        got.sort();
        assert_eq!(got, ["-oPasswordAuthentication=no", "-oPasswordAuthentication=yes"]);
    }
}

#[test]
fn bash_and_zsh_snippets_are_valid_shell() {
    for (shell, flag) in [("bash", "bash"), ("zsh", "zsh")] {
        if !have(shell) {
            continue;
        }
        for (name, _, body) in complete_me::snippets::for_shell(flag) {
            // empty bodies are where you type; fill them so the structure can be checked
            let code = body.replace("\n    \n", "\n    :\n").replace("\n    \n", "\n    :\n").replace("{\n    \n}", "{\n    :\n}").replace(") ;;", ") :;;").replace('%', "x");
            // ",and" / ",or" end with an operator on purpose: the next command is yours
            let code = if code.ends_with("&& ") || code.ends_with("|| ") { code + "true" } else { code }.replace("CMD", "echo").replace("FILE", "f").replace("HOST", "h").replace("PORT", "1");
            let out = Command::new(shell).args(["-n", "-c", &code]).output().unwrap();
            assert!(out.status.success(), "{shell} {name}: {code}\n{}", String::from_utf8_lossy(&out.stderr));
        }
    }
}

impl Env {
    fn executable(&self, name: &str, script: &str) {
        let path = self.tmp.join("bin").join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{script}\n")).unwrap();
        std::fs::set_permissions(path, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    }

    fn live(&self, current: &str, words: &[&str]) -> String {
        let result = self.cmd("complete-me").args(["live", "--current", current, "--"]).args(words).output().unwrap();
        assert!(result.status.success());
        assert!(result.stderr.is_empty());
        String::from_utf8(result.stdout).unwrap()
    }
}

#[test]
fn runtime_providers_use_the_current_slot_and_scope() {
    let e = Env::new("live");
    let containers = r#"
        case "$*" in
            '--context=work container ls --all --format '*|'--connection=work container ls --all --format '*)
                printf '%s\n' 'web|ab0123|app:dev|Up five minutes'
                [ ! -e changed ] || printf '%s\n' 'worker|ab0124|app:dev|Exited'
                ;;
            'container ls --format '*) printf '%s\n' 'web|ab0123|app:dev|Up five minutes' ;;
            'image ls --format '*) printf '%s\n' 'app|dev|cd3456|30 MB|yesterday' ;;
            'network ls --format '*) printf '%s\n' 'frontend|bridge|ef5678' ;;
            *) exit 2 ;;
        esac
    "#;
    e.executable("docker", containers);
    e.executable("podman", containers);
    e.executable("zerotier-cli", r#"
        [ "$*" = '-D/tmp/zt -j listnetworks' ] || exit 2
        printf '%s\n' '[{"nwid":"1234567890abcdef","name":"Lab network","status":"OK","type":"PRIVATE","assignedAddresses":["192.0.2.5/24"]}]'
    "#);
    e.executable("kubectl", r#"
        case "$*" in
            'config get-contexts -o name') printf '%s\n' work ;;
            '--context=work --namespace=lab get pods --request-timeout=300ms -o json')
                printf '%s\n' '{"items":[{"metadata":{"name":"app-0","namespace":"lab"},"status":{"phase":"Running"}}]}'
                ;;
            *) exit 2 ;;
        esac
    "#);
    e.executable("systemctl", r#"
        [ "$*" = '--user list-units --all --plain --no-legend --no-pager' ] || exit 2
        printf '%s\n' 'worker.service loaded active running Worker'
    "#);
    e.executable("brew", r#"
        [ "$*" = 'list --cask' ] || exit 2
        printf '%s\n' 'editor'
    "#);
    std::fs::write(e.tmp.join("package.json"), r#"{"name":"@example/local","scripts":{"build":"exit 99",".helper":"exit 98"}}"#).unwrap();
    for tool in ["npm", "pnpm", "yarn", "bun"] {
        assert!(e.live("b", &[tool, "run"]).contains("build\tpackage.json script · @example/local"));
    }
    assert!(e.live(".", &["pnpm", "run"]).is_empty());
    assert!(e.live("", &["yarn", "run", "--binaries-only"]).is_empty());
    assert!(e.live("ed", &["brew", "uninstall", "--cask"]).contains("editor\tInstalled cask"));
    let docker = ["docker", "--context", "work", "container", "logs", "--tail", "20"];
    assert!(e.live("we", &docker).contains("web\tapp:dev · Up five minutes · ab0123"));
    assert!(e.live("wo", &docker).is_empty());
    std::fs::write(e.tmp.join("changed"), "").unwrap();
    assert!(e.live("wo", &docker).contains("worker\tapp:dev · Exited"));
    assert!(e.live("we", &["podman", "--connection=work", "logs"]).contains("web\t"));
    assert!(e.live("app", &["docker", "run", "-it", "-e", "X=1"]).contains("app:dev\tcd3456 · 30 MB"));
    assert!(e.live("--network=", &["docker", "run"]).contains("--network=frontend\tbridge"));
    assert!(e.live("web", &["docker", "exec", "-it", "web"]).is_empty());
    assert!(e.live("1234", &["zerotier-cli", "-D/tmp/zt", "leave"]).contains("1234567890abcdef\tLab network · OK · PRIVATE · 192.0.2.5/24"));
    assert!(e.live("--context=", &["kubectl", "get", "pods"]).contains("--context=work\t"));
    assert!(e.live("po/app", &["kubectl", "--context", "work", "-nlab", "logs"]).contains("po/app-0\tpods · lab · Running"));
    assert!(e.live("app", &["kubectl", "--server=https://example.invalid", "get", "pods"]).is_empty());
    assert!(e.live("--user-unit=w", &["journalctl"]).contains("--user-unit=worker.service\tloaded active running Worker"));

    if have("fish") {
        assert!(e.fish_with("docker --context work container logs we", None).iter().any(|(v, d)| v == "web" && d.contains("Up five minutes")));
        assert!(e.fish("zerotier-cli -D/tmp/zt leave 1234").contains(&"1234567890abcdef".into()));
        assert!(e.fish("kubectl get pods --context=").contains(&"--context=work".into()));
    }
    if have("bash") {
        assert!(e.bash("docker --context work logs we", &[], None, false).contains(&"web".into()));
        assert!(e.bash("docker run --network=", &["docker", "run", "--network", "="], None, false).contains(&"frontend".into()));
    }
    if have("zsh") {
        assert!(e.zsh("docker --context work logs we", None).contains(&("cm-live".into(), "web".into())));
    }
}

#[test]
fn tmux_targets_from_an_isolated_live_server() {
    if !have("tmux") {
        return;
    }
    let e = Env::new("tmux");
    let socket = format!("cm-e2e-{}", std::process::id());
    let tmux = |args: &[&str]| {
        e.cmd("tmux").env_remove("TMUX").args(["-L", &socket, "-f", "/dev/null"]).args(args).output().unwrap()
    };
    struct Server<'a>(&'a str);
    impl Drop for Server<'_> {
        fn drop(&mut self) {
            let _ = Command::new("tmux").env_remove("TMUX").args(["-L", self.0, "kill-server"]).output();
        }
    }
    assert!(tmux(&["new-session", "-d", "-s", "cm-build", "-n", "editor", "sleep 30"]).status.success());
    let _server = Server(&socket);
    let args = ["tmux", "-L", &socket, "a", "-t"];
    let got = e.live("cm-", &args);
    assert!(got.contains("cm-build\t1 windows · detached"), "{got:?}");
    let got = e.live("cm-build:", &args);
    assert!(got.contains("cm-build:0\teditor"), "{got:?}");
    let got = e.live("-tcm-build:", &["tmux", "-L", &socket, "selectw"]);
    assert!(got.contains("-tcm-build:0\teditor"), "{got:?}");
    let got = e.live("cm-build:", &["tmux", "-L", &socket, "swapp", "-s"]);
    assert!(got.contains("cm-build:0.0\teditor"), "{got:?}");
    assert!(tmux(&["new-window", "-d", "-t", "cm-build", "-n", "new-window", "sleep 30"]).status.success());
    let got = e.live("cm-build:", &["tmux", "-L", &socket, "select-window", "-t"]);
    assert!(got.contains("cm-build:1\tnew-window"), "{got:?}");
    if have("bash") && have("python3") {
        let runtime = Path::new(env!("CARGO_MANIFEST_DIR")).join("shell/bash/complete_me.bash");
        let result = e.cmd("python3").env("HOME", &e.tmp).env_remove("TMUX")
            .args([&harness("cursor.py"), "bash", runtime.to_str().unwrap(),
                &format!("tmux -L {socket} a -t cm-build:"), "", "list-tab"])
            .output().unwrap();
        assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
        let output = String::from_utf8_lossy(&result.stdout);
        assert!(output.lines().any(|line| line.split_whitespace().eq(["0", "1"])), "{output:?}");
    }
    if have("fish") {
        // Fish's bundled completion may connect independently of our -L-aware provider.
        let out = tmux(&["display-message", "-p", "#{socket_path},#{pid},0"]);
        assert!(out.status.success());
        let context = String::from_utf8(out.stdout).unwrap();
        let fish = |line: &str| e.fish_with_env(line, None, &[("TMUX", context.trim())]);
        assert!(fish(&format!("tmux -L {socket} a -t cm-")).iter().any(|(v, _)| v == "cm-build"));
        assert!(fish(&format!("tmux -L {socket} selectw -tcm-build:")).iter().any(|(v, _)| v == "-tcm-build:1"));
    }
}

impl Env {
    fn edit_with_tab(&self, shell: &str, left: &str, right: &str) -> String {
        let rt = Path::new(env!("CARGO_MANIFEST_DIR")).join("shell").join(shell).join(format!("complete_me.{shell}"));
        let rt = rt.to_str().unwrap();
        let mut command = if shell == "zsh" {
            let mut c = self.cmd("zsh");
            c.args(["-f", &harness("zsh_capture.zsh"), rt, left, right]);
            c
        } else {
            let mut c = self.cmd("python3");
            c.args([&harness("cursor.py"), shell, rt, left, right]);
            c
        };
        let result = command.env("HOME", &self.tmp).output().unwrap();
        assert!(result.status.success(), "{shell}: {}", String::from_utf8_lossy(&result.stderr));
        let stdout = String::from_utf8(result.stdout).unwrap();
        let buffer = if shell == "zsh" {
            stdout.lines().find_map(|line| line.strip_prefix("BUFFER\t")).unwrap_or("")
        } else {
            stdout.trim_end_matches('\n')
        };
        buffer.to_string()
    }
}

#[test]
fn calculator_tab_inserts_answers_and_remembers_them_in_each_editor() {
    if !have("python3") {
        return;
    }
    let e = Env::new("calculator");
    for shell in ["bash", "zsh", "fish"] {
        if !have(shell) {
            continue;
        }
        let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("shell").join(shell).join(format!("complete_me.{shell}"));
        let edit = |left, right, action| {
            let result = e.cmd("python3")
                .env("HOME", &e.tmp)
                .args([&harness("cursor.py"), shell, runtime.to_str().unwrap(), left, right, action])
                .output().unwrap();
            assert!(result.status.success(), "{shell}: {}", String::from_utf8_lossy(&result.stderr));
            String::from_utf8(result.stdout).unwrap().trim().to_owned()
        };
        assert_eq!(edit("2*34=", "", "calculator"), "2*34=68\nans/4=17", "{shell}");
        assert_eq!(edit("2*34=68 (", "/3)^2=", ""), "2*34=68 (68/3)^2=", "{shell}");
    }
}

#[test]
fn newly_created_argparse_commands_complete_without_restarting_the_shell() {
    if !have("python3") {
        return;
    }
    for (shell, generic) in [
        ("bash", ""), ("bash", "_minimal"), ("bash", "_comp_complete_minimal"),
        ("zsh", ""), ("fish", ""),
    ] {
        if !have(shell) {
            continue;
        }
        let e = Env::new(&format!("new-command-{shell}{generic}"));
        let fixture = e.tmp.join("script.fixture");
        std::fs::write(&fixture, "#!/usr/bin/env python3\nimport argparse\np = argparse.ArgumentParser()\np.add_argument('--format', choices=['json', 'text'])\n").unwrap();
        std::fs::set_permissions(&fixture, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("shell").join(shell).join(format!("complete_me.{shell}"));
        let result = e.cmd("python3")
            .env("HOME", &e.tmp)
            .env("COMPLETE_ME_NO_PREGEN", "1")
            .env("CM_TEST_GENERIC_COMPLETION", generic)
            .args([&harness("cursor.py"), shell, runtime.to_str().unwrap(), "ship-demo --fo", "", "new-command"])
            .output().unwrap();
        assert!(result.status.success(), "{shell}: {}", String::from_utf8_lossy(&result.stderr));
        let edited = String::from_utf8(result.stdout).unwrap();
        assert!(edited.starts_with("ship-demo --format"), "{shell} {generic}: {edited:?}");
    }
}

#[test]
fn tabs_inside_ssh_commands_preserve_following_arguments() {
    let e = Env::new("cursor");
    let ssh_dir = e.tmp.join(".ssh");
    std::fs::create_dir(&ssh_dir).unwrap();
    std::fs::write(ssh_dir.join("identity key"), "").unwrap();
    let host = " user@ssh.com";
    for (left, right, expected) in [
        ("ssh -oPubkeyAuth", " -oIdentityFile= user@ssh.com", "ssh -oPubkeyAuthentication"),
        ("ssh -oIdentityFile=", host, "ssh -oIdentityFile="),
        ("ssh -oIdentityFile=~/.ssh/", host, "ssh -oIdentityFile="),
    ] {
        for shell in ["bash", "fish", "zsh"] {
            if !have(shell) || (shell != "zsh" && !have("python3")) {
                continue;
            }
            let buffer = e.edit_with_tab(shell, left, right);
            assert!(buffer.starts_with(expected) && buffer.ends_with(right), "{shell}: {buffer:?}");
            if left.contains("IdentityFile") {
                let line = complete_me::extras::split_line(&buffer);
                assert_eq!(line.words[1].replace("\\ ", " "), "-oIdentityFile=~/.ssh/identity key", "{shell}: {buffer:?}");
            }
        }
    }
}

#[test]
fn fish_enter_translates_standalone_bash_exports() {
    if !have("fish") || !have("python3") {
        return;
    }
    let e = Env::new("fish-exports");
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR")).join("shell/fish/complete_me.fish");
    let bin = e.tmp.join("bin");
    let submit = |line, setting: Option<&str>| {
        let mut cmd = e.cmd("python3");
        cmd.args([&harness("cursor.py"), "fish", runtime.to_str().unwrap(), line, "", "enter"])
            .env("HOME", &e.tmp)
            .env_remove("CM_EXPORT")
            .env_remove("CM_EXPORT_EMPTY")
            .env_remove("CM_TEST_CUSTOM_EXPORT")
            .env_remove("complete_me_no_bash");
        if let Some(setting) = setting {
            cmd.env(setting, "1");
        }
        let output = cmd.output().unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        String::from_utf8(output.stdout).unwrap()
    };

    assert_eq!(submit(r#"export CM_EXPORT="two words" CM_EXPORT_EMPTY= PATH="/cm-example:$PATH""#, None),
        "translated\ntwo words||/cm-example\nabsent\n");
    assert_eq!(submit(r#"export CM_EXPORT="two words""#, None),
        format!("translated\ntwo words|unset|{}\nfound\n", bin.display()));
    assert_eq!(submit(r#" export CM_EXPORT="two words""#, None),
        format!("translated\ntwo words|unset|{}\nabsent\n", bin.display()));
    assert_eq!(submit("export CM_EXPORT=original", Some("CM_TEST_CUSTOM_EXPORT")),
        format!("native\ncustom|unset|{}\nabsent\n", bin.display()));
    assert_eq!(submit("export CM_EXPORT=original", Some("complete_me_no_bash")),
        format!("native\noriginal|unset|{}\nabsent\n", bin.display()));
    assert_eq!(submit("export CM_EXPORT=original; set -gx CM_EXPORT_EMPTY native", None),
        format!("native\noriginal|native|{}\nabsent\n", bin.display()));
}

#[test]
fn fish_environment_preview_apply_and_undo_are_session_local() {
    if !have("fish") {
        return;
    }
    let e = Env::new("env-edit");
    std::fs::write(e.tmp.join("profile.env"), r#"
export CM_ENV_ADD='$(printf never) ; literal'
CM_ENV_CHANGE="new\nline"
CM_ENV_DROP=
unset CM_ENV_LIST
"#).unwrap();
    std::fs::write(e.tmp.join("invalid.env"), "CM_ENV_EXTRA=set\nprintf 'not an assignment'\n").unwrap();
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR")).join("shell/fish/complete_me.fish");
    let out = e.cmd("fish").env("HOME", &e.tmp)
        .env_remove("CM_ENV_ADD").env_remove("CM_ENV_EXTRA").env_remove("CM_ENV_STDIN")
        .args(["--no-config", "-c", r#"
            source $argv[1]
            set -gx CM_ENV_CHANGE old
            set -gx CM_ENV_DROP old
            set -gx --path CM_ENV_LIST dir1 dir2
            complete-me env diff profile.env; or exit 2
            test "$CM_ENV_CHANGE" = old; or exit 3
            printf 'preview complete\n'
            complete-me env apply --yes profile.env; or exit 4
            /bin/sh -c 'test "$CM_ENV_ADD" = '\''$(printf never) ; literal'\'' && test "${CM_ENV_DROP-unset}" = "" && test "${CM_ENV_LIST-unset}" = unset'; or exit 5
            test "$CM_ENV_CHANGE" = "new"\n"line"; or exit 6
            if complete-me env apply --yes invalid.env
                exit 7
            end
            set -q CM_ENV_EXTRA; and exit 8
            complete-me env undo; or exit 9
            set -q CM_ENV_ADD; and exit 10
            set -qgx --path CM_ENV_LIST; or exit 11
            test (count $CM_ENV_LIST) -eq 2; or exit 12
            test "$CM_ENV_LIST[1]" = dir1; and test "$CM_ENV_LIST[2]" = dir2; or exit 13
            test "$CM_ENV_CHANGE" = old; and test "$CM_ENV_DROP" = old; or exit 14
            if complete-me env undo
                exit 15
            end
            complete-me env apply --yes - < profile.env; or exit 16
            complete-me env undo; or exit 17
            printf 'CM_ENV_STDIN=piped\n' | complete-me env apply --yes -; or exit 18
            test "$CM_ENV_STDIN" = piped; or exit 19
        "#, runtime.to_str().unwrap()]).output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{:?}: {stdout}\n{stderr}", out.status);
    assert!(stdout.contains("add CM_ENV_ADD\nchange CM_ENV_CHANGE\nchange CM_ENV_DROP\nremove CM_ENV_LIST\npreview complete"));
    assert!(!stdout.contains("printf never") && !stdout.contains("new\nline"));
    assert_eq!(stderr.lines().count(), 2, "{stderr}");
    assert!(stderr.contains("invalid or protected assignment on line 2"));
    assert!(stderr.contains("no changes to undo"));
    if have("python3") {
        std::fs::write(e.tmp.join("confirm.env"), "CM_EXPORT=reviewed\n").unwrap();
        for (response, expected) in [("decline", "unset"), ("confirm", "reviewed")] {
            let output = e.cmd("python3").env("HOME", &e.tmp).env_remove("CM_EXPORT").args([
                &harness("cursor.py"), "fish", runtime.to_str().unwrap(), "complete-me env apply confirm.env", "", response,
            ]).output().unwrap();
            assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
            assert!(String::from_utf8_lossy(&output.stdout).lines().nth(1).unwrap().starts_with(&format!("{expected}|")));
        }
    }
}

#[test]
fn path_why_reports_aliases_and_shadowed_executables() {
    let e = Env::new("path-why");
    let first = e.tmp.join("bin");
    let second = e.tmp.join("second bin");
    std::fs::create_dir(&second).unwrap();
    e.executable("cm-selected", "exit 21");
    std::fs::copy(first.join("cm-selected"), second.join("cm-selected")).unwrap();
    let path = format!("{}:{}:{}:{}", first.display(), second.display(), first.display(), e.path);
    for (shell, flags) in [("fish", "--no-config"), ("bash", "--norc"), ("zsh", "-f")] {
        if !have(shell) {
            continue;
        }
        let runtime = Path::new(env!("CARGO_MANIFEST_DIR")).join("shell").join(shell).join(format!("complete_me.{shell}"));
        let aliases = if shell == "bash" { "shopt -s expand_aliases" } else { "" };
        let script = format!("source {}\n{aliases}\ncomplete-me path why cm-selected\nalias cm-selected='printf example'\ncomplete-me path why cm-selected\ncomplete-me path why printf", fq(runtime.to_str().unwrap()));
        let out = e.cmd(shell).env("HOME", &e.tmp).env("PATH", &path)
            .env("COMPLETE_ME_NO_PLUGINS", "1").args([flags, "-c", &script]).output().unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(out.status.success(), "{shell}: {:?}", out);
        assert!(stdout.starts_with(&format!("Selected: {}/cm-selected\n  PATH[1] selected: ", first.display())), "{shell}: {stdout}");
        assert!(stdout.contains(&format!("PATH[2] shadowed: {}/cm-selected", second.display())), "{shell}: {stdout}");
        assert!(stdout.contains("PATH[3] duplicates PATH[1]"), "{shell}: {stdout}");
        assert!(stdout.contains("Selected: alias cm-selected"), "{shell}: {stdout}");
        assert!(stdout.contains("Selected: builtin printf"), "{shell}: {stdout}");
    }
}

#[test]
fn project_recipes_complete_live_slots_and_expand_in_the_editor() {
    let e = Env::new("recipes");
    std::fs::create_dir(e.tmp.join(".complete-me")).unwrap();
    std::fs::write(e.tmp.join(".complete-me/recipes.json"), r#"{
        "logs": ["docker", "compose", "logs", "-f", "{service}"],
        "repeat": ["printf", "%s", "{message}", "{message}"]
    }"#).unwrap();
    e.executable("docker", r#"
        [ "$1 $2 $3 $4 $5" = 'compose --profile=* config --services --no-env-resolution' ] || exit 21
        printf '%s\n' worker website
    "#);
    let subdir = e.tmp.join("nested");
    std::fs::create_dir(&subdir).unwrap();
    let output = e.cmd("complete-me").current_dir(&subdir).args(["recipe", "expand", "repeat", "a;b $(echo never)"]).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&output.stdout), "'printf' %s 'a;b $(echo never)' 'a;b $(echo never)'\n");
    let output = e.cmd("complete-me").args(["recipe", "expand", "logs"]).output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    for shell in ["fish", "bash", "zsh"] {
        if !have(shell) || !have("python3") {
            continue;
        }
        let line = e.edit_with_tab(shell, "complete-me recipe expand logs wo", " ");
        assert!(line.starts_with("complete-me recipe expand logs worker"), "{shell}: {line:?}");
        let runtime = Path::new(env!("CARGO_MANIFEST_DIR")).join("shell").join(shell).join(format!("complete_me.{shell}"));
        let edit = |line: &str| {
            let out = e.cmd("python3").env("HOME", &e.tmp).args([
                &harness("cursor.py"), shell, runtime.to_str().unwrap(), line, "", "recipe",
            ]).output().unwrap();
            assert!(out.status.success(), "{shell}: {}", String::from_utf8_lossy(&out.stderr));
            String::from_utf8(out.stdout).unwrap().trim_end_matches('\n').to_string()
        };
        assert_eq!(edit(" complete-me recipe expand logs worker"), " 'docker' compose logs -f worker", "{shell}");
        let compound = "complete-me recipe expand logs worker | cat";
        assert_eq!(edit(compound), compound, "{shell}");
    }
}

#[test]
fn fish_can_manage_its_functions_aliases_and_path() {
    if !have("fish") {
        return;
    }
    let e = Env::new("fish-management");
    let rt = Path::new(env!("CARGO_MANIFEST_DIR")).join("shell/fish/complete_me.fish");
    let script = format!(r#"
        source {}
        function cm_saved; echo saved; end
        complete-me func --save cm_saved; or exit
        complete-me func cm_created 'echo created'; or exit
        cm_created; or exit
        alias cm_native 'echo native'
        complete-me alias --list; or exit
        complete-me func --list; or exit
        complete-me path add --global ./bin; or exit
        complete-me path list | string match -q -- '*cm-e2e-fish-management*/bin'; or exit 5
    "#, fq(rt.to_str().unwrap()));
    let out = e.cmd("fish").args(["--no-config", "-c", &script]).output().unwrap();
    assert!(out.status.success(), "{:?}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("\ncreated\n") && stdout.contains("cm_native") && stdout.contains("\ncm_saved\n"), "{stdout:?}");
    assert!(e.tmp.join("config/fish/functions/cm_saved.fish").is_file());
}

#[test]
fn git_refs_come_from_the_selected_worktree() {
    if !have("git") {
        return;
    }
    let e = Env::new("git-refs");
    let repo = e.tmp.join("nested");
    std::fs::create_dir(&repo).unwrap();
    let git = |args: &[&str]| {
        let out = e.cmd("git").current_dir(&repo).args(args).output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    };
    git(&["init", "-b", "trunk"]);
    git(&["-c", "user.name=Test", "-c", "user.email=test@example.invalid", "-c", "commit.gpgSign=false", "commit", "--allow-empty", "-m", "Initial work"]);
    git(&["branch", "feature/editing"]);
    let got = e.live("feature", &["git", "-C", "nested", "switch"]);
    assert!(got.contains("feature/editing\t") && got.contains("Initial work"), "{got:?}");
}

#[test]
fn git_config_completes_current_values_in_the_selected_scope() {
    if !have("git") {
        return;
    }
    let e = Env::new("git-config");
    let email = "global-fixture@example.invalid";
    let global = format!("[user]\n\temail = {email}\n\tname = \"Example & Co\"\n[http]\n\textraHeader = fixture-private-value\n[url \"https://embedded:fixture-private@example.invalid\"]\n\tinsteadOf = test:\n");
    std::fs::write(e.tmp.join(".gitconfig"), &global).unwrap();
    let repo = e.tmp.join("nested");
    std::fs::create_dir(&repo).unwrap();
    assert!(e.cmd("git").arg("init").arg(&repo).output().unwrap().status.success());
    let local = "[core]\n\trepositoryformatversion = 0\n[user]\n\temail = local-fixture@example.invalid\n";
    std::fs::write(repo.join(".git/config"), local).unwrap();
    std::fs::write(e.tmp.join("selected config"), "[user]\n\temail = file-fixture@example.invalid\n").unwrap();

    let live = |line: &str| {
        let output = e.cmd("complete-me").env("HOME", &e.tmp).env_remove("GIT_CONFIG").env_remove("GIT_CONFIG_GLOBAL").env("GIT_CONFIG_NOSYSTEM", "1")
            .args(["live", "--line", line]).output().unwrap();
        assert!(output.status.success(), "{line}");
        String::from_utf8(output.stdout).unwrap()
    };
    for (line, expected) in [
        ("git con", "config\tGit command"),
        ("git config set --glo", "--global\tUser config"),
        ("git config get user.em", "user.email\t"),
        ("git -C nested config --global user.email ", email),
        ("git -C nested config set --global user.email ", email),
        ("git -C nested config set user.email ", "local-fixture@example.invalid"),
        ("git config set --file='selected config' user.email ", "file-fixture@example.invalid"),
        ("git config set --type=bo", "--type=bool\t"),
    ] {
        let rows = live(line);
        assert!(rows.contains(expected), "{line}: {rows:?}");
    }
    assert!(live("git config set --global http.extraHeader ").is_empty());
    assert!(live("git config set --global --local user.email ").is_empty());
    assert!(live("git config get --global user.email ").is_empty());
    assert!(!live("git config get --global url.").contains("fixture-private"));
    let report = e.cmd("complete-me").env("HOME", &e.tmp).env_remove("GIT_CONFIG_GLOBAL").args([
        "live", "--explain", "--json", "--line", "git config set --global user.email ",
    ]).output().unwrap();
    let report: serde_json::Value = serde_json::from_slice(&report.stdout).unwrap();
    assert_eq!(report["sources"][0]["slot"], "current Git config value");
    assert!(!report.to_string().contains(email));

    let version = e.cmd("git").arg("--version").output().unwrap();
    let version = String::from_utf8(version.stdout).unwrap();
    let mut version = version.split_ascii_whitespace().nth(2).unwrap().split('.');
    let modern = (version.next().unwrap().parse::<u32>().unwrap(), version.next().unwrap().parse::<u32>().unwrap()) >= (2, 46);
    assert_eq!(live("git config se").contains("set\tWrite a setting"), modern);
    for shell in ["fish", "bash", "zsh"] {
        if !have(shell) || (shell != "zsh" && !have("python3")) {
            continue;
        }
        let line = if modern { "git config set --global user.email " } else { "git config --global user.email " };
        let buffer = e.edit_with_tab(shell, line, " ");
        assert!(buffer.starts_with(&format!("{line}{email}")), "{shell}: {buffer:?}");
        let name_line = line.replace("user.email", "user.name");
        let buffer = e.edit_with_tab(shell, &name_line, " ");
        let tokens = complete_me::extras::split_line(&format!("{} ", buffer.trim_end()));
        assert_eq!(tokens.words.last().map(String::as_str), Some("Example & Co"), "{shell}: {buffer:?}");
        let protected = line.replace("user.email", "http.extraHeader");
        let buffer = e.edit_with_tab(shell, &protected, " ");
        assert_eq!(buffer, format!("{protected} "), "{shell}");
        if modern {
            let buffer = e.edit_with_tab(shell, "git config se", " --global user.email");
            assert!(buffer.starts_with("git config set") && buffer.ends_with("--global user.email"), "{shell}: {buffer:?}");
        }
    }
    assert_eq!(std::fs::read_to_string(e.tmp.join(".gitconfig")).unwrap(), global);
    assert_eq!(std::fs::read_to_string(repo.join(".git/config")).unwrap(), local);
}

#[test]
fn git_repository_objects_complete_in_their_own_slots() {
    if !have("git") { return }
    let e = Env::new("git-inventories");
    let repo = e.tmp.join("nested");
    std::fs::create_dir_all(repo.join("src")).unwrap();
    let git = |args: &[&str]| {
        let out = e.cmd("git").current_dir(&repo).env("HOME", &e.tmp)
            .env_remove("GIT_DIR").env_remove("GIT_WORK_TREE").env_remove("GIT_INDEX_FILE")
            .args(args).output().unwrap();
        assert!(out.status.success(), "{args:?}: {}", String::from_utf8_lossy(&out.stderr));
    };
    git(&["init", "-b", "trunk"]);
    git(&["config", "user.name", "Fixture"]);
    git(&["config", "user.email", "test@example.invalid"]);
    std::fs::write(repo.join("src/fixture path"), "original\n").unwrap();
    git(&["add", "--", "src"]);
    git(&["-c", "user.name=Fixture", "-c", "user.email=test@example.invalid", "-c", "commit.gpgSign=false", "commit", "-m", "Fixture commit"]);
    git(&["branch", "feature/topic"]);
    git(&["remote", "add", "origin", "https://fixture.invalid/team/example"]);
    git(&["update-ref", "refs/remotes/origin/trunk", "HEAD"]);
    git(&["update-ref", "refs/remotes/origin/incoming", "HEAD"]);
    git(&["config", "branch.trunk.remote", "origin"]);
    std::fs::write(repo.join("src/fixture path"), "stash me\n").unwrap();
    git(&["stash", "push", "-m", "Fixture stash"]);
    let worktree = e.tmp.join("other worktree");
    git(&["worktree", "add", worktree.to_str().unwrap(), "feature/topic"]);
    let untracked = repo.join("untracked file");
    std::fs::write(&untracked, "data\n").unwrap();
    std::fs::write(repo.join(".gitmodules"), "[submodule \"library\"]\npath = vendor/library\nurl = https://fixture.invalid/team/lib\n").unwrap();
    for (current, words, expected) in [
        ("trunk..fea", &["log"][..], "trunk..feature/topic\t"),
        ("trunk...fea", &["diff"][..], "trunk...feature/topic\t"),
        ("--source=fea", &["restore"][..], "--source=feature/topic\t"),
        ("HEAD:src/fi", &["show"][..], "HEAD:src/fixture path\t"),
        ("untr", &["add"][..], "untracked file\tChanged or untracked path"),
        ("src/fi", &["restore", "--"][..], "src/fixture path\tTracked path"),
        ("ori", &["remote", "set-url"][..], "origin\tGit remote"),
        ("inc", &["fetch", "origin"][..], "incoming\t"),
        ("inc", &["push", "--delete", "origin"][..], "incoming\t"),
        ("", &["worktree", "remove"][..], "other worktree\tfeature/topic"),
        ("stash", &["stash", "pop"][..], "stash@{0}\tOn trunk: Fixture stash"),
        ("ven", &["submodule", "update", "--"][..], "vendor/library\tConfigured submodule"),
        ("", &["config", "set", "branch.trunk.remote"][..], "origin\tCurrent value"),
    ] {
        let words = [&["git", "-C", "nested"][..], words].concat();
        let rows = e.live(current, &words);
        assert!(rows.contains(expected), "{words:?}: {rows:?}");
    }
    assert!(e.live("--output:whatever", &["git", "-C", "nested", "show"]).is_empty());
    assert!(untracked.is_file() && worktree.is_dir());
    for shell in ["fish", "bash", "zsh"] {
        if !have(shell) || (shell != "zsh" && !have("python3")) { continue }
        let edit = e.edit_with_tab(shell, "git -C nested log trunk..fea", " -- src");
        assert!(edit.contains("trunk..feature/topic") && edit.ends_with(" -- src"), "{shell}: {edit:?}");
    }
}

#[test]
fn container_selectors_and_management_targets_use_read_only_queries() {
    let e = Env::new("container-management");
    e.executable("docker", r#"
        case "$1 $2 $3 $4" in
            '--config=isolated context ls --format') printf '%s\n' 'context-fixture|true' ;;
            'context ls --format {{.Name}}|{{.Current}}') printf '%s\n' 'context-fixture|true' ;;
            '--context=work volume ls --format') printf '%s\n' 'data-fixture|local' ;;
            '--context=work network ls --format') printf '%s\n' 'net-fixture|bridge|abcde' ;;
            '--context=work image ls --format') printf '%s\n' 'image-fixture|latest|abcde|2MB|1 day' ;;
            '--context=work container ls --all') printf '%s\n' 'worker-fixture|abcde|image:dev|Up' ;;
            '--context=work container inspect --format')
                [ "$5" = '{{.HostConfig.RestartPolicy.Name}}|{{.HostConfig.RestartPolicy.MaximumRetryCount}}' ] && [ "$6" = -- ] && [ "$7" = worker-fixture ] || exit 7
                printf '%s\n' 'on-failure|3' ;;
            '--context=work plugin ls --format') printf '%s\n' 'driver-fixture|true' ;;
            '--context=work service ls --format') printf '%s\n' 'web-fixture|2/2' ;;
            *) printf '%s\n' "Unexpected Docker query: $*" >> unexpected-query; exit 7 ;;
        esac
    "#);
    e.executable("podman", r#"
        case "$1 $2 $3 $4" in
            'system connection list --format') printf '%s\n' 'lab-fixture|true' ;;
            '--connection=lab pod ps --format') printf '%s\n' 'pod-fixture|Running' ;;
            '--connection=lab secret ls --format') printf '%s\n' 'key-fixture|abc123' ;;
            'machine list --format {{.Name}}|{{.Running}}') printf '%s\n' 'vm-fixture|true' ;;
            *) printf '%s\n' "Unexpected Podman query: $*" >> unexpected-query; exit 7 ;;
        esac
    "#);
    for (line, expected) in [
        ("docker --config isolated --context=co", "--context=context-fixture\tDocker context"),
        ("docker context use co", "context-fixture\tDocker context"),
        ("docker --context work run --volume=da", "--volume=data-fixture:\tlocal"),
        ("docker --context work run --mount=type=volume,source=da", "--mount=type=volume,source=data-fixture\tlocal"),
        ("docker --context work run --volumes-from=wor", "--volumes-from=worker-fixture\timage:dev"),
        ("docker --context work inspect --type=network ne", "net-fixture\tbridge"),
        ("docker --context work ps --filter=ancestor=ima", "--filter=ancestor=image-fixture:latest\t"),
        ("docker --context work update worker-fixture --restart=", "--restart=on-failure:3\tCurrent restart policy"),
        ("docker --context work service scale we", "web-fixture=\tSwarm service"),
        ("docker --context work plugin inspect dr", "driver-fixture\tDocker plugin"),
        ("docker --context work service logs we", "web-fixture\tSwarm service"),
        ("podman --connection la", "lab-fixture\tPodman connection"),
        ("podman system connection rename la", "lab-fixture\tPodman connection"),
        ("podman --connection lab pod stop po", "pod-fixture\tPodman pod"),
        ("podman --connection lab pod ps --filter=name=po", "--filter=name=pod-fixture\tPodman pod"),
        ("podman --connection lab run --pod=po", "--pod=pod-fixture\tPodman pod"),
        ("podman --connection lab run --secret=ke", "--secret=key-fixture\tPodman secret"),
        ("podman --connection lab secret inspect ke", "key-fixture\tPodman secret"),
        ("podman machine ssh vm", "vm-fixture\tPodman machine"),
    ] {
        let output = e.cmd("complete-me").env_remove("COMPLETE_ME_NO_REMOTE").args(["live", "--line", line]).output().unwrap();
        let output = String::from_utf8(output.stdout).unwrap();
        assert!(output.contains(expected), "{line}: {output:?}");
    }
    let offline = e.cmd("complete-me").env("COMPLETE_ME_NO_REMOTE", "1").env("DOCKER_HOST", "ssh://fixture.invalid")
        .args(["live", "--line", "docker --context co"]).output().unwrap();
    assert!(String::from_utf8_lossy(&offline.stdout).contains("context-fixture"));
    assert!(!e.tmp.join("unexpected-query").exists());
    for shell in ["fish", "bash", "zsh"] {
        if !have(shell) || (shell != "zsh" && !have("python3")) { continue }
        let edit = e.edit_with_tab(shell, "docker --context work run --mount=type=volume,source=da", ",target=/app img");
        assert_eq!(edit, "docker --context work run --mount=type=volume,source=data-fixture,target=/app img", "{shell}");
    }
}

#[test]
fn coverage_catalog_distinguishes_supported_slots_and_limits() {
    let e = Env::new("live-coverage");
    let output = e.cmd("complete-me").args(["live", "--coverage", "--json", "podman"]).output().unwrap();
    assert!(output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let slots = report["slots"].as_array().unwrap();
    assert!(slots.iter().all(|slot| slot["tool"] == "podman" && slot["source"].is_string()));
    assert!(slots.iter().any(|slot| slot["example"] == "podman run --pod="));
    assert!(report["limits"].as_array().unwrap().iter().any(|limit| limit["status"] == "requires-execution-or-filesystem-access"));
}

#[test]
fn generated_shells_share_path_resolution() {
    use complete_me::model::{Command as Model, Opt};
    let e = Env::new("shared-path");
    let mut root = Model::with_source("gramtest", "test");
    root.takes_files = false;
    root.options.push(Opt { shorts: vec!["C".into()], longs: vec!["config".into()], arg: Some("PATH".into()), kind: "dir".into(), ..Opt::default() });
    root.options.push(Opt { shorts: vec!["t".into()], arg: Some("VALUE".into()), ..Opt::default() });
    root.options.push(Opt { longs: vec!["theme".into()], arg: Some("VALUE".into()), arg_optional: true, ..Opt::default() });
    let mut group = Model::new("group");
    group.aliases.push("g".into());
    group.takes_files = false;
    group.options.push(Opt { shorts: vec!["t".into()], ..Opt::default() });
    let mut leaf = Model::new("build");
    leaf.aliases.push("b".into());
    leaf.takes_files = false;
    leaf.options.push(Opt { longs: vec!["mode".into()], arg: Some("WHEN".into()), values: vec![("auto".into(), "Pick automatically".into())], kind: "enum".into(), ..Opt::default() });
    group.subcommands.insert(leaf.name.clone(), leaf);
    root.subcommands.insert(group.name.clone(), group);
    root.subcommands.insert("unrelated".into(), Model::new("unrelated"));

    let base = e.tmp.join("cache/complete-me");
    for (file, source) in [
        ("fish/gramtest.fish", render::fish(&root).unwrap()),
        ("bash/gramtest", render::bash(&root).unwrap()),
        ("zsh/gramtest.zsh", render::zsh(&root).unwrap()),
    ] {
        std::fs::write(base.join(file), source).unwrap();
    }
    e.executable("gramtest", "exit 42");
    let command = "gramtest -qC unrelated --theme g -t b --mode=";
    if have("fish") {
        assert!(e.fish(command).contains(&"--mode=auto".into()));
        assert!(e.fish("gramtest --config unrelated g -t b --mo").contains(&"--mode".into()));
        assert!(!e.fish("gramtest -- g").contains(&"group".into()));
    }
    if have("bash") {
        assert!(e.bash(command, &[], None, false).contains(&"auto".into()));
        assert!(e.bash("gramtest --config unrelated g -t b --mo", &[], None, false).contains(&"--mode".into()));
        assert!(!e.bash("gramtest -- g", &[], None, false).contains(&"group".into()));
        std::fs::create_dir(e.tmp.join("directory name")).unwrap();
        assert!(e.bash("gramtest --config=dir", &[], None, false).contains(&"directory name".into()));
    }
    if have("zsh") {
        let zsh = e.zsh(command, None);
        assert!(zsh.iter().any(|(_, candidate)| candidate == "auto" || candidate == "--mode=auto"), "{zsh:?}");
        let zsh = e.zsh("gramtest --config unrelated g -t b --mo", None);
        assert!(zsh.iter().any(|(_, candidate)| candidate == "--mode"), "{zsh:?}");
    }
}

#[test]
fn new_live_providers_and_diagnostics() {
    let e = Env::new("live-expanded");
    e.executable("docker", r#"
        case "$*" in
            '--context=work compose -f test.yml --profile=* config --services --no-env-resolution') printf '%s\n' web worker ;;
            'compose --file=test.yml config --profiles --no-env-resolution'|'compose -f test.yml config --profiles --no-env-resolution') printf '%s\n' debug ;;
            *) exit 6 ;;
        esac
    "#);
    e.executable("docker-compose", r#"
        [ "$*" = '--profile=* config --services --no-env-resolution' ] || exit 6
        printf '%s\n' worker
    "#);
    e.executable("gh", r#"
        printf '%s\n' "$1 $2" >> gh-calls
        case "$*" in
            '--repo=example/first pr list --state all --limit 50 --json number,title,state,updatedAt')
                printf '%s\n' '[{"number":42,"title":"Parser improvements","state":"OPEN","updatedAt":"2026-11-20"}]' ;;
            '--repo=example/second pr list --state all --limit 50 --json number,title,state,updatedAt')
                printf '%s\n' '[{"number":43,"title":"Site copy","state":"CLOSED"}]' ;;
            '--repo=example/first issue list --state open --limit 50 --json number,title,state,updatedAt')
                printf '%s\n' '[{"number":24,"title":"Docs follow-up","state":"OPEN"}]' ;;
            *) exit 7 ;;
        esac
    "#);
    e.executable("just", r#"
        [ "$*" = '-f build.just --list --list-heading= --list-prefix=' ] || exit 6
        printf '%s\n' 'build target # Build the project'
    "#);
    e.executable("task", "exit 97");
    std::fs::write(e.tmp.join("Taskfile.yml"), r#"
version: '3'
vars:
  UNSAFE_TEST:
    sh: touch unexpected-task-execution
tasks:
  build:
    desc: Build the project
    aliases: [b]
  hidden:
    internal: true
    cmds: [echo hidden]
"#).unwrap();
    assert!(e.live("we", &["docker", "--context=work", "compose", "-f", "test.yml", "logs"]).contains("web\tCompose service"));
    assert!(e.live("--profile=d", &["docker", "compose", "--file=test.yml"]).contains("--profile=debug\tCompose profile"));
    assert!(e.live("d", &["docker", "compose", "-f", "test.yml", "--profile"]).contains("debug\tCompose profile"));
    assert!(e.live("work", &["docker-compose", "up"]).contains("worker\tCompose service"));
    assert!(e.live("web", &["docker", "compose", "exec", "web"]).is_empty());
    assert!(e.live("b", &["just", "-f", "build.just"]).contains("build\tBuild the project"));
    assert_eq!(e.live("b", &["task"]), "build\tBuild the project\nb\tAlias for build\n");
    assert!(e.live("hidden", &["task"]).is_empty());
    assert!(!e.tmp.join("unexpected-task-execution").exists());

    let explain = |line: &str| {
        let out = e.cmd("complete-me").args(["live", "--explain", "--json", "--line", line]).output().unwrap();
        assert!(out.status.success() && out.stderr.is_empty());
        serde_json::from_slice::<serde_json::Value>(&out.stdout).unwrap()
    };
    let first = ["gh", "pr", "view", "-R", "example/first"];
    assert!(e.live("4", &first).contains("42\tParser improvements · OPEN"));
    let report = explain("gh pr view -R example/first 4");
    assert_eq!(report["sources"][0]["status"], "hit");
    assert_eq!(report["matches"], 1);
    assert!(e.live("4", &["gh", "pr", "view", "-Rexample/second"]).contains("43\tSite copy"));
    assert!(e.live("", &["gh", "issue", "close", "-R", "example/first"]).contains("24\tDocs follow-up"));
    assert_eq!(std::fs::read_to_string(e.tmp.join("gh-calls")).unwrap().lines().count(), 3);
    use std::os::unix::fs::PermissionsExt;
    let cache = e.tmp.join("cache/complete-me/data/gh");
    assert_eq!(std::fs::metadata(&cache).unwrap().permissions().mode() & 0o777, 0o700);
    for entry in std::fs::read_dir(cache).unwrap() {
        let file = std::fs::File::open(entry.unwrap().path()).unwrap();
        assert_eq!(file.metadata().unwrap().permissions().mode() & 0o777, 0o600);
        file.set_times(std::fs::FileTimes::new().set_modified(
            std::time::SystemTime::now() - std::time::Duration::from_secs(30)
        )).unwrap();
    }
    let report = explain("gh pr view -R example/first 4");
    assert_eq!(report["sources"][0]["status"], "success");
    assert_eq!(std::fs::read_to_string(e.tmp.join("gh-calls")).unwrap().lines().count(), 4);

    assert_eq!(explain("gh pr create --title ")["status"], "no-provider-for-slot");
    assert_eq!(explain("unknown-tool ")["status"], "unsupported-tool");
    assert_eq!(explain("docker --context work compose -f test.yml logs nothing")["status"], "no-matches");

    e.executable("zerotier-cli", "printf '%s\\n' 'private-token-value' >&2; exit 9");
    let failed = explain("zerotier-cli -Tprivate-token-value leave ");
    assert_eq!(failed["sources"][0]["status"], "failed");
    assert_eq!(failed["sources"][0]["exit_code"], 9);
    assert!(!failed.to_string().contains("private-token-value"));
    e.executable("systemctl", "sleep 2");
    assert_eq!(explain("systemctl status ")["sources"][0]["status"], "timeout");
}

#[test]
fn required_tools_when_running_full_shell_matrix() {
    if std::env::var_os("COMPLETE_ME_REQUIRE_TEST_TOOLS").is_none() {
        return;
    }
    for tool in ["fish", "bash", "zsh", "tmux", "python3", "git", "ssh", "man"] {
        assert!(have(tool), "the full test suite requires {tool}");
    }
    assert!(Command::new("fish").args(["-c", "test (string split -f1 . -- $version) -ge 4"])
        .status().unwrap().success(), "the full test suite requires fish 4+");
    assert!(Command::new("bash").args(["-c", "(( BASH_VERSINFO[0] > 4 || (BASH_VERSINFO[0] == 4 && BASH_VERSINFO[1] >= 2) ))"])
        .status().unwrap().success(), "the full test suite requires Bash 4.2+");
    assert!(!complete_me::probe::man_text("ssh_config").is_empty(), "the full test suite requires the ssh_config manual");
}

#[test]
fn tab_inside_tokens_preserves_quotes_pipeline_and_existing_value() {
    let e = Env::new("editing");
    let ssh = e.tmp.join(".ssh");
    std::fs::create_dir(&ssh).unwrap();
    for key in ["laboratory.key", "identity key.pem", "delta.txt"] {
        std::fs::write(ssh.join(key), "").unwrap();
    }
    for shell in ["bash", "fish", "zsh"] {
        if !have(shell) || (shell != "zsh" && !have("python3")) {
            continue;
        }
        let completion = e.edit_with_tab(shell, "printf x | cargo-like --color=a", "ys | cat");
        assert!(completion.contains("--color=always") && completion.ends_with("ys | cat"), "{shell}: {completion:?}");
        for (left, right, contains) in [
            ("ssh -oIdentityFile=~/.ssh/la", ".key user@example.com", "/laboratory.key"),
            ("echo λ | ssh -o \"IdentityFile=~/.ssh/iden", ".pem\" user@example.com", "identity"),
            ("ssh -oPub", "Authentication=yes user@example.com", "-oPubkeyAuthentication=yes"),
            ("ssh -oIdentityFile=~/.ssh/de", ".key user@example.com", "/de.key"),
        ] {
            let result = e.edit_with_tab(shell, left, right);
            assert!(result.contains(contains) && result.ends_with(right), "{shell}: {result:?}");
            if left.contains("echo") {
                assert_eq!(result, r#"echo λ | ssh -o "IdentityFile=~/.ssh/identity\\ key.pem" user@example.com"#, "{shell}");
            }
        }
    }
}

#[test]
fn scope_selectors_affect_queries_and_are_redacted_from_diagnostics() {
    let e = Env::new("scope");
    e.executable("docker", r#"
        case "$1" in
            completion) exit 0 ;;
            container)
                [ "${DOCKER_CONTEXT-}" = private-lab ] || [ "${DOCKER_HOST-}" = unix:///tmp/lab.sock ] || exit 6
                printf '%s\n' 'worker|ab123|app:dev|Up' ;;
            *) exit 9 ;;
        esac
    "#);
    e.executable("kubectl", r#"
        [ "$1" = config ] || exit 8
        [ "$KUBECONFIG" = /tmp/kube-test ] || exit 6
        printf '%s\n' 'lab-context'
    "#);
    e.executable("gh", r#"
        printf '%s\n' "$GH_HOST $1" >> calls
        printf '%s\n' '[{"number":3,"title":"Example"}]'
    "#);
    let say = |line: &str, offline: bool| {
        let mut cmd = e.cmd("complete-me");
        if offline { cmd.env("COMPLETE_ME_NO_REMOTE", "1"); }
        else { cmd.env_remove("COMPLETE_ME_NO_REMOTE"); }
        let output = cmd.args(["live", "--explain", "--json", "--line",line]).output().unwrap();
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap()
    };
    let report = say("env -u DOCKER_HOST DOCKER_CONTEXT=private-lab docker logs w", false);
    assert_eq!(report["status"], "matches");
    assert_eq!(report["sources"][0]["environment_selectors"], serde_json::json!(["DOCKER_HOST","DOCKER_CONTEXT"]));
    assert_eq!(report["runtime"]["wrappers"], serde_json::json!(["env"]));
    assert!(report["sources"][0]["elapsed_ms"].is_u64());
    assert!(!report.to_string().contains("private-lab"));
    assert_eq!(say("sudo docker logs w", false)["status"], "scope-blocked");
    assert_eq!(say("DOCKER_HOST=$runtime docker logs w", false)["status"], "scope-blocked");
    assert_eq!(say("PATH=/tmp docker logs w", false)["sources"][0]["reason"], "execution-environment");
    assert_eq!(say("DOCKER_CONTEXT=private-lab docker logs w", true)["status"], "remote-disabled");
    assert_eq!(say("DOCKER_HOST=unix:///tmp/lab.sock docker logs w", true)["status"], "matches");
    assert_eq!(say("KUBECONFIG=/tmp/kube-test kubectl --context=", true)["status"], "matches");
    assert_eq!(say("kubectl get pod ", true)["status"], "remote-disabled");
    assert_eq!(say("GH_HOST=lab.example GH_REPO=example/project gh issue view ", true)["status"], "remote-disabled");
    assert!(!e.tmp.join("calls").exists());
    assert_eq!(say("GH_HOST=lab.example GH_REPO=example/project gh issue view ", false)["status"], "matches");
    assert_eq!(say("GH_HOST=lab.example GH_REPO=example/project gh issue view ", false)["sources"][0]["status"], "hit");
    assert_eq!(say("GH_HOST=other.example GH_REPO=example/project gh issue view ", false)["sources"][0]["status"], "success");
    let calls = std::fs::read_to_string(e.tmp.join("calls")).unwrap();
    assert!(calls.contains("lab.example") && calls.contains("other.example"));
    assert_eq!(calls.lines().count(), 2);

    if have("fish") && have("python3") {
        assert_eq!(e.edit_with_tab("fish", "env DOCKER_CONTEXT=private-lab docker logs w", "ee"), "env DOCKER_CONTEXT=private-lab docker logs wee");
        assert!(e.edit_with_tab("fish", "env DOCKER_CONTEXT=private-lab docker logs w", " | cat").contains("logs worker | cat"));
    }
}

#[test]
fn doctor_reads_binding_and_handler_from_each_running_shell() {
    let e = Env::new("doctor-shell");
    let scripts = [
        ("fish", "--no-config", "complete -c cargo-like -n __complete_me_at -s x"),
        ("bash", "--norc", "source \"$XDG_CACHE_HOME/complete-me/bash/cargo-like\""),
        ("zsh", "-f", "source \"$XDG_CACHE_HOME/complete-me/zsh/cargo-like.zsh\""),
    ];
    for (shell, flag, load_generated) in scripts {
        if !have(shell) { continue; }
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("shell").join(shell).join(format!("complete_me.{shell}"));
        let script = format!("source {}\n{load_generated}\ncomplete-me doctor --command cargo-like --json", fq(path.to_str().unwrap()));
        let result = e.cmd(shell).args([flag,"-ic",&script]).output().unwrap();
        assert!(result.status.success(), "{shell}: {}", String::from_utf8_lossy(&result.stderr));
        let report: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
        assert_eq!(report["shell"], shell);
        assert_eq!(report["binding"], "complete-me", "{report}");
        assert_eq!(report["handler"], "generated", "{report}");
        assert_eq!(report["cache"]["present"], true);
        assert_eq!(report["cache"]["loaded"], true);
        assert!(report["live"].is_null());
    }
}

#[test]
fn proxmox_local_slots_include_guest_metadata() {
    let e = Env::new("proxmox-local");
    e.executable("pct", r#"
case "$*" in
  'list')
    printf '%-10s %-10s %-12s %-20s\n' VMID Status Lock Name \
      100 running '' dns-prod 105 stopped backup archive 110 running '' backup
    ;;
  'config 100')
    printf '%s\n' 'net0: name=eth0,bridge=vmbr0,ip=192.0.2.100/24,ip6=fd00::100/64' \
      'net1: name=eth1,ip=dhcp,ip6=fe80::100/64' 'description: ip=198.51.100.27'
    ;;
  'listsnapshot 100') printf '%s\n' '`-> pre-upgrade' ' `-> current' ;;
  *) exit 1 ;;
esac
"#);
    e.executable("qm", r#"
case "$*" in
  'list') printf '%s\n' 'VMID NAME STATUS MEM(MiB) BOOTDISK(GiB) PID' '410 build-vm running 1024 20.00 421' ;;
  'guest cmd 410 network-get-interfaces')
    printf '%s\n' '[{"ip-addresses":[{"ip-address":"127.0.0.1"},{"ip-address":"198.51.100.41"}]}]'
    ;;
  *) exit 1 ;;
esac
"#);
    e.executable("pvesm", r#"
case "$*" in
  'status') printf '%s\n' 'Name Type Status Total Used Available' 'fast zfspool active 1000 500 500' ;;
  'list fast') printf '%s\n' 'Volid Format Type Size VMID' 'fast:vm-410-disk-0 raw images 5368709120 410' ;;
  *) exit 1 ;;
esac
"#);
    e.executable("pvesh", r#"
[ "$*" = 'get /nodes --output-format json' ] || exit 1
printf '%s\n' '[{"node":"pve2","status":"online"}]'
"#);
    let enter = e.live("10", &["pct", "enter"]);
    assert_eq!(enter, "100\tdns-prod · running · local node\n");
    assert!(e.live("10", &["pct", "start"]).contains("105\tarchive · stopped"));
    assert!(e.live("11", &["pct", "enter"]).contains("110\tbackup · running"));
    assert_eq!(e.live("", &["pct", "rollback", "100"]), "pre-upgrade\tSnapshot of 100\n");
    assert!(e.live("", &["pct", "snapshot", "100"]).is_empty());
    assert!(e.live("", &["pct", "clone", "100"]).is_empty());
    assert!(e.live("", &["pct", "rollback", "100; ls"]).is_empty());
    assert_eq!(e.live("", &["qm", "migrate", "410"]), "pve2\tonline\n");
    assert_eq!(e.live("4", &["qm", "guest", "cmd"]), "410\tbuild-vm · running · local node\n");
    assert!(e.live("", &["qm", "disk", "move", "410", "scsi0"]).contains("fast\tzfspool · active"));
    assert!(e.live("fast:", &["pvesm", "path"]).contains("fast:vm-410-disk-0\traw · images"));
    assert!(e.live("fas", &["pvesm", "list", "--vmid", "410"]).contains("fast\t"));
    assert!(e.live("fast:", &["pvesm", "list"]).is_empty());
    for (line, address) in [("pct enter 100", "192.0.2.100, fd00::100"), ("qm terminal 41", "198.51.100.41")] {
        let result = e.cmd("complete-me").args(["live", "--line", line])
            .env("COMPLETE_ME_PROXMOX_IP", "1").output().unwrap();
        assert!(result.status.success());
        let output = String::from_utf8(result.stdout).unwrap();
        assert!(output.trim_end().ends_with(address), "{output:?}");
    }
    let multi = e.cmd("complete-me").args(["live", "--line", "pct enter "])
        .env("COMPLETE_ME_PROXMOX_IP", "1").output().unwrap();
    assert!(!String::from_utf8(multi.stdout).unwrap().contains("192.0.2"));
}

#[test]
fn proxmox_ssh_only_queries_configured_hosts_and_requests_a_console_pty() {
    let e = Env::new("proxmox-ssh");
    let config = e.tmp.join("config/complete-me");
    std::fs::create_dir_all(&config).unwrap();
    std::fs::write(config.join("proxmox-hosts"), "lab-pve # node\nother-pve\n-oSomeOption=foo\n").unwrap();
    let log = e.tmp.join("ssh-calls");
    e.executable("ssh", r#"
printf '%s\n' "$@" >> ssh-calls
for last do :; done
case "$last" in
  "'pct' 'list'")
    printf '%-10s %-10s %-12s %-20s\n' VMID Status Lock Name 100 running '' dns-prod 105 stopped '' archive
    ;;
  "'pct' 'enter' '100'") printf '%s\n' "console attached"; exit 17 ;;
esac
"#);
    assert_eq!(e.live("lab", &["proxmox"]), "lab-pve\tProxmox SSH node\n");
    assert!(e.live("p", &["proxmox", "lab-pve"]).contains("pct\t"));
    assert!(e.live("", &["proxmox", "unknown", "pct", "enter"]).is_empty());
    assert!(e.live("", &["proxmox", "-oSomeOption=foo", "pct", "enter"]).is_empty());
    assert!(e.live("", &["proxmox", "lab-pve", "pct", "rollback", "100; echo test"]).is_empty());
    let blocked = e.cmd("complete-me").args(["live", "--explain", "--json", "--line", "proxmox lab-pve pct enter "])
        .env("COMPLETE_ME_NO_REMOTE", "1").output().unwrap();
    let report: serde_json::Value = serde_json::from_slice(&blocked.stdout).unwrap();
    assert_eq!(report["status"], "remote-disabled");
    assert!(!log.exists());
    assert_eq!(e.live("10", &["proxmox", "lab-pve", "pct", "enter"]), "100\tdns-prod · running · lab-pve\n");
    let argv = std::fs::read_to_string(&log).unwrap();
    assert!(argv.starts_with("-T\n") && argv.contains("\nBatchMode=yes\n") && argv.contains("\nStrictHostKeyChecking=yes\n"));
    assert!(argv.ends_with("--\nlab-pve\n'pct' 'list'\n"), "{argv:?}");
    for shell in ["bash", "fish", "zsh"] {
        if !have(shell) || (shell != "zsh" && !have("python3")) {
            continue;
        }
        let buffer = e.edit_with_tab(shell, "proxmox lab-pve pct enter 10", " --keep-env");
        assert_eq!(buffer, "proxmox lab-pve pct enter 100 --keep-env", "{shell}");
    }
    std::fs::remove_file(&log).unwrap();
    let entered = e.cmd("complete-me").args(["proxmox", "lab-pve", "pct", "enter", "100"]).output().unwrap();
    assert_eq!(entered.status.code(), Some(17));
    assert_eq!(entered.stdout, b"console attached\n");
    assert_eq!(std::fs::read_to_string(&log).unwrap(), "-t\n-o\nStrictHostKeyChecking=yes\n--\nlab-pve\n'pct' 'enter' '100'\n");

    std::fs::remove_file(&log).unwrap();
    let arg = "one'two; $TEXT";
    let changed = e.cmd("complete-me").args(["proxmox", "lab-pve", "pct", "set", "100", "--description", arg])
        .output().unwrap();
    assert!(changed.status.success());
    assert_eq!(
        std::fs::read_to_string(&log).unwrap(),
        "-T\n-o\nStrictHostKeyChecking=yes\n--\nlab-pve\n'pct' 'set' '100' '--description' 'one'\"'\"'two; $TEXT'\n",
    );
}

#[test]
fn zfs_references_complete_datasets_snapshots_pools_and_mounted_paths() {
    let e = Env::new("zfs-path");
    e.executable("zfs", r#"
case "$*" in
  'list -H -t filesystem,volume -o name,type,mountpoint')
    printf 'tank/app\tfilesystem\t%s/mounted\ntank/vm-410-disk-0\tvolume\t-\n' "$PWD"
    ;;
  'list -H -t filesystem -o name,type,mountpoint') printf 'tank/app\tfilesystem\t%s/mounted\n' "$PWD" ;;
  'list -H -t snapshot -o name,type,mountpoint') printf 'tank/app@before\tsnapshot\t-\n' ;;
  'list -H -t filesystem -o mountpoint,mounted tank/app') printf '%s/mounted\tyes\n' "$PWD" ;;
  'list -H -t filesystem -o mountpoint,mounted tank/offline') printf '%s/mounted\tno\n' "$PWD" ;;
  *) exit 1 ;;
esac
"#);
    e.executable("zpool", r#"[ "$*" = 'list -H -o name,health' ] && printf 'tank\tONLINE\n'"#);
    std::fs::create_dir_all(e.tmp.join("mounted/logs")).unwrap();
    std::fs::write(e.tmp.join("mounted/logs/daemon log"), "").unwrap();
    std::fs::write(e.tmp.join("outside"), "").unwrap();
    std::os::unix::fs::symlink("..", e.tmp.join("mounted/escape")).unwrap();
    assert!(e.live("tank/", &["zfs", "list"]).contains("tank/vm-410-disk-0\tvolume"));
    assert_eq!(e.live("tank/app@", &["zfs", "rollback"]), "tank/app@before\tsnapshot\n");
    assert_eq!(e.live("tank/app@", &["zfs", "list", "-t", "snapshot"]), "tank/app@before\tsnapshot\n");
    assert!(e.live("tank/app@", &["zfs", "list", "-t", "filesystem"]).is_empty());
    assert_eq!(e.live("t", &["zpool", "status"]), "tank\tONLINE\n");
    assert!(e.live("tank/", &["complete-me", "zfs-path"]).contains("tank/app\tfilesystem"));
    assert!(e.live("lo", &["complete-me", "zfs-path", "tank/app"]).contains("logs/\t"));
    assert_eq!(e.live("logs/dae", &["complete-me", "zfs-path", "tank/app"]), "logs/daemon log\tPath in tank/app\n");
    if have("fish") {
        assert!(e.fish_with("complete-me zfs-path tank/app logs/dae", None).iter()
            .any(|(value, _)| value == "logs/daemon log"));
    }
    for relative in ["../", "/tmp/", "escape/"] {
        assert!(e.live(relative, &["complete-me", "zfs-path", "tank/app"]).is_empty());
        assert!(!e.cmd("complete-me").args(["zfs-path", "tank/app", relative]).output().unwrap().status.success());
    }
    assert!(!e.cmd("complete-me").args(["zfs-path", "tank/offline"]).output().unwrap().status.success());
    let result = e.cmd("complete-me").args(["zfs-path", "tank/app", "logs/daemon log"]).output().unwrap();
    assert!(result.status.success());
    assert_eq!(String::from_utf8(result.stdout).unwrap().trim_end(), e.tmp.join("mounted/logs/daemon log").canonicalize().unwrap().to_str().unwrap());
}

#[test]
fn dd_uses_partition_bytes_and_device_geometry() {
    let e = Env::new("dd-geometry");
    e.executable("lsblk", r#"
[ "$*" = '--json --bytes --paths --list --output NAME,TYPE,PKNAME,START,SIZE,LOG-SEC,PHY-SEC,RO,MOUNTPOINTS' ] || exit 1
cat <<'JSON'
{"blockdevices":[
 {"name":"/dev/demo","type":"disk","size":50000000,"log-sec":4096,"phy-sec":4096,"ro":false},
 {"name":"/dev/demo1","pkname":"/dev/demo","type":"part","start":2048,"size":10000000,"log-sec":4096,"phy-sec":4096,"ro":false,"mountpoints":["/mnt/example"]},
 {"name":"/dev/demo2","pkname":"/dev/demo","type":"part","start":"4096","log-sec":"4096","phy-sec":4096,"ro":true},
 {"name":"/dev/other1","pkname":"/dev/other","type":"part","start":6144,"log-sec":512,"phy-sec":4096}
]}
JSON
"#);
    let files = e.live("if=/dev/demo", &["dd"]);
    assert!(files.contains("if=/dev/demo1\tpart · starts 1048576 B · 4096/4096 B logical/physical"));
    assert!(files.contains("mounted") && files.contains("read-only"));
    assert!(!e.live("of=/dev/demo", &["dd"]).contains("of=/dev/demo2"));
    let offset = e.live("skip=", &["dd", "if=/dev/demo", "bs=1M"]);
    assert!(offset.contains("skip=1048576B\t/dev/demo1") && offset.contains("skip=2097152B\t/dev/demo2"));
    assert!(!offset.contains("other1"));
    assert!(e.live("seek=", &["dd", "if=/dev/demo", "of=/dev/demo1"]).starts_with("seek=0\tStart of /dev/demo1"));
    assert!(e.live("ibs=", &["dd", "if=/dev/demo"]).starts_with("ibs=4096\t"));
    assert!(e.live("start=", &["dd", "if=/dev/demo"]).is_empty());
    assert!(e.live("skip=", &["dd", "if=local.img"]).is_empty());
}

#[test]
fn network_helpers_offer_local_interfaces_and_safe_report_flags() {
    let e = Env::new("interfaces");
    let loopback = if cfg!(target_os = "macos") { "lo0" } else { "lo" };
    for words in [&["tcpdump", "-i"][..], &["wireshark", "-i"], &["tshark", "-i"], &["ip", "-6", "route", "show", "dev"], &["route", "add", "dev"]] {
        assert!(e.live(loopback, words).starts_with(&format!("{loopback}\t")));
    }
    assert!(e.live("ma", &["ip", "r", "show", "table"]).contains("main\tRoute table"));
    assert!(e.live("-o", &["nmap"]).contains("-oX\t"));
    assert!(e.live("", &["nmap"]).is_empty());
    assert!(e.live("--script", &["nmap"]).is_empty());
    if have("fish") {
        e.executable("tcpdump", "exit 73");
        assert!(e.fish("tcpdump -i lo").contains(&loopback.to_string()));
    }
}

#[test]
fn byte_offsets_use_the_file_after_the_cursor() {
    let e = Env::new("byte-offset");
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    png.extend(116_u32.to_be_bytes());
    png.extend(b"tEXt");
    png.extend([0; 120]);
    png.extend(0_u32.to_be_bytes());
    png.extend(b"IEND");
    png.extend([0; 4]);
    std::fs::write(e.tmp.join("sample file.png"), png).unwrap();
    e.executable("hexdump", "exit 73");
    let lines = e.cmd("complete-me").args(["bytes", "sample file.png"]).output().unwrap();
    assert!(lines.status.success());
    let lines = String::from_utf8(lines.stdout).unwrap();
    assert!(lines.contains("0x88\t136\tPNG IEND chunk"));
    let right = " 'sample file.png'";
    for shell in ["bash", "fish", "zsh"] {
        if have(shell) && (shell == "zsh" || have("python3")) {
            assert_eq!(e.edit_with_tab(shell, "hexdump -C -s 1", right), format!("hexdump -C -s 136{right}"), "{shell}");
        }
    }
    let bad = e.cmd("complete-me").args(["extras", "--shell", "bash", "--right", " $(touch sentinel)", "--", "hexdump -s "]).output().unwrap();
    assert!(!String::from_utf8(bad.stdout).unwrap().contains("Beginning of file"));
    assert!(!e.tmp.join("sentinel").exists());
    assert!(!e.cmd("complete-me").args(["bytes", "."]).output().unwrap().status.success());
}

#[test]
fn built_in_shortcuts_respect_existing_names() {
    let e = Env::new("shortcuts");
    for shell in ["bash", "fish", "zsh"] {
        if !have(shell) { continue; }
        let runtime = Path::new(env!("CARGO_MANIFEST_DIR")).join("shell").join(shell).join(format!("complete_me.{shell}"));
        let (flags, script) = if shell == "fish" {
            (&["--no-config", "-c"][..], format!(r#"
function cm-doctor; printf '%s\n' preserve-me; end
source {}
cm-doctor
type -q cm-live; and type -q path-why; and type -q zfs-path; and type -q cm-recipe
"#, fq(runtime.to_str().unwrap())))
        } else {
            let flags = if shell == "bash" { &["--noprofile", "--norc", "-c"][..] } else { &["-f", "-c"][..] };
            let find = if shell == "bash" { "shopt -s expand_aliases; type -t" } else { "whence -w" };
            (flags, format!(r#"
cm-doctor() {{ printf '%s\n' preserve-me; }}
source {}
cm-doctor
{find} cm-live path-why zfs-path cm-recipe >/dev/null
"#, complete_me::util::sq(runtime.to_str().unwrap())))
        };
        let result = e.cmd(shell).args(flags).arg(script).output().unwrap();
        assert!(result.status.success(), "{shell}: {}", String::from_utf8_lossy(&result.stderr));
        assert_eq!(result.stdout, b"preserve-me\n", "{shell}");
    }
}

#[test]
fn zfs_path_resolves_local_pool_roots_and_legacy_mounts() {
    let e = Env::new("zfs-legacy");
    e.executable("zfs", r#"
case "$*" in
  'list -H -t filesystem -o mountpoint,mounted tank') printf '%s/pool\tyes\n' "$PWD" ;;
  'list -H -t filesystem -o mountpoint,mounted tank/legacy') printf 'legacy\tyes\n' ;;
  *) exit 1 ;;
esac
"#);
    e.executable("findmnt", r#"
[ "$*" = '--json --types zfs --source tank/legacy --output TARGET' ] || exit 1
printf '{"filesystems":[{"target":"%s/legacy path"}]}\n' "$PWD"
"#);
    e.executable("mount", r#"printf 'tank/legacy on %s/legacy path (zfs, local)\n' "$PWD""#);
    for (dataset, directory) in [("tank", "pool"), ("tank/legacy", "legacy path")] {
        let directory = e.tmp.join(directory);
        std::fs::create_dir(&directory).unwrap();
        let result = e.cmd("complete-me").args(["zfs-path", dataset]).output().unwrap();
        assert!(result.status.success(), "{dataset}: {}", String::from_utf8_lossy(&result.stderr));
        assert_eq!(String::from_utf8_lossy(&result.stdout).trim_end(), directory.canonicalize().unwrap().to_str().unwrap());
        assert!(e.live(dataset, &["zfs-path", "unmounted"]).is_empty());
    }
}

#[test]
fn fish_keeps_container_metadata_when_vendor_completions_erase_registrations() {
    if !have("fish") { return; }
    let e = Env::new("container-vendor");
    let native = e.tmp.join("config/fish/completions");
    std::fs::create_dir_all(&native).unwrap();
    for tool in ["docker", "podman"] {
        std::fs::write(native.join(format!("{tool}.fish")), format!(
            "complete -c {tool} -e\ncomplete -c {tool} -a 'vendor-action'\n"
        )).unwrap();
        e.executable(tool, r#"
[ "$1 $2" = "container ls" ] || exit 1
printf '%s\n' 'worker|012345678abc|example:stable|Up 1 minute'
"#);
        let runtime = Path::new(env!("CARGO_MANIFEST_DIR")).join("shell/fish/complete_me.fish");
        let result = e.cmd("fish").args([
            "--no-config", "-c",
            "set -g fish_complete_path $argv[2] $fish_complete_path; source $argv[1]; complete -C $argv[3]; complete -C $argv[4]",
            runtime.to_str().unwrap(), native.to_str().unwrap(),
            &format!("{tool} vendor"), &format!("{tool} container inspect wor"),
        ]).output().unwrap();
        assert!(result.status.success() && result.stderr.is_empty(), "{}", String::from_utf8_lossy(&result.stderr));
        let rows = String::from_utf8(result.stdout).unwrap();
        assert!(rows.lines().any(|line| line == "vendor-action"), "{rows:?}");
        assert!(rows.lines().any(|line| line.starts_with("worker\t") && line.contains("example:stable") && line.contains("Up 1 minute")), "{rows:?}");
    }
}

#[test]
fn proxmox_cluster_routes_only_unique_mapped_guests_to_current_owners() {
    let e = Env::new("prox-cluster");
    let config = e.tmp.join("config/complete-me");
    std::fs::create_dir_all(&config).unwrap();
    std::fs::write(config.join("proxmox-hosts"), "seed\nssh-a\nssh-b\n").unwrap();
    std::fs::write(config.join("proxmox-clusters"),
        "lab seed pve-a=ssh-a pve-b=ssh-b\ninvalid seed pve-a=other\ndup seed node=ssh-a\ndup seed node=ssh-b\n").unwrap();
    e.executable("ssh", r#"
mode=$1
host= request=
for arg do host=$request; request=$arg; done
printf '%s\n' "$mode $host $request" >> cluster.calls
case "$host $request" in
  "seed 'pvesh' 'get' '/cluster/resources' '--type' 'vm' '--output-format' 'json'")
    if [ -f relocated ]; then
      printf '%s\n' '[{"vmid":410,"type":"qemu","node":"pve-a","name":"builder","status":"running"}]'
      exit
    fi
    cat <<'JSON'
[{"vmid":100,"type":"lxc","node":"pve-a","name":"dns","status":"running"},
 {"vmid":101,"type":"lxc","node":"pve-b","name":"stopped","status":"stopped"},
 {"vmid":102,"type":"lxc","node":"unknown","name":"unmapped","status":"running"},
 {"vmid":103,"type":"lxc","node":"pve-a","name":"duplicate","status":"running"},
 {"vmid":103,"type":"lxc","node":"pve-b","name":"duplicate","status":"running"},
 {"vmid":410,"type":"qemu","node":"pve-b","name":"builder","status":"running"}]
JSON
    ;;
  "ssh-b 'sh' '-c' "*" 'complete-me' 'pve-b' 'qm' 'terminal' '410'"|"ssh-a 'sh' '-c' "*" 'complete-me' 'pve-a' 'qm' 'terminal' '410'")
    test "$mode" = -t && printf 'console\n' ;;
  "ssh-a 'pct' 'config' '100'") printf 'net0: name=eth0,ip=192.0.2.100/24\n' ;;
  "ssh-a 'pct' 'listsnapshot' '100'") printf '%s\n' '`-> backup' ;;
  *) exit 91 ;;
esac
"#);
    assert_eq!(e.live("lab", &["proxmox"]), "lab\tProxmox cluster\n");
    assert!(e.live("dup", &["proxmox"]).is_empty());
    assert!(e.live("invalid", &["proxmox"]).is_empty());
    assert_eq!(e.live("10", &["proxmox", "lab", "pct", "enter"]), "100\tdns · running · pve-a\n");
    assert_eq!(e.live("41", &["proxmox", "lab", "qm", "terminal"]), "410\tbuilder · running · pve-b\n");
    assert_eq!(e.live("", &["proxmox", "lab", "pct", "rollback", "100"]), "backup\tSnapshot of 100\n");
    let ips = e.cmd("complete-me").args(["live", "--line", "proxmox lab pct enter 100"])
        .env("COMPLETE_ME_PROXMOX_IP", "1").output().unwrap();
    assert!(String::from_utf8(ips.stdout).unwrap().contains("192.0.2.100"));
    let entered = e.cmd("complete-me").args(["proxmox", "lab", "qm", "terminal", "410"])
        .env("COMPLETE_ME_NO_REMOTE", "1").output().unwrap();
    assert!(entered.status.success());
    assert_eq!(entered.stdout, b"console\n");
    for id in ["101", "102", "103", "410", "100;other"] {
        let blocked = e.cmd("complete-me").args(["proxmox", "lab", "pct", "enter", id]).output().unwrap();
        assert!(!blocked.status.success(), "{id}");
    }
    std::fs::write(e.tmp.join("relocated"), "").unwrap();
    assert!(e.cmd("complete-me").args(["proxmox", "lab", "qm", "terminal", "410"]).output().unwrap().status.success());
    let calls = std::fs::read_to_string(e.tmp.join("cluster.calls")).unwrap();
    let routed = calls.lines().filter(|line| line.starts_with("-t")).collect::<Vec<_>>();
    assert_eq!(routed.len(), 2);
    assert!(routed[0].starts_with("-t ssh-b 'sh' '-c' ") && routed[0].ends_with("'pve-b' 'qm' 'terminal' '410'"));
    assert!(routed[1].starts_with("-t ssh-a 'sh' '-c' ") && routed[1].ends_with("'pve-a' 'qm' 'terminal' '410'"));
    let offline = e.cmd("complete-me").args(["live", "--explain", "--json", "--line", "proxmox lab pct enter "])
        .env("COMPLETE_ME_NO_REMOTE", "1").output().unwrap();
    assert_eq!(serde_json::from_slice::<serde_json::Value>(&offline.stdout).unwrap()["status"], "remote-disabled");
    assert_eq!(std::fs::read_to_string(e.tmp.join("cluster.calls")).unwrap(), calls);
}

#[test]
fn kube_routes_qualified_names_and_refuses_conflicting_scope() {
    let e = Env::new("kube-route");
    e.executable("kubectl", r#"
printf '%s\n' "$*" >> kubectl.calls
case "$*" in
 'config get-contexts -o name') printf '%s\n' dev prod ;;
 '--context=prod get namespaces --request-timeout=750ms -o json')
   printf '%s\n' '{"items":[{"metadata":{"name":"team-a"},"status":{"phase":"Active"}}]}' ;;
 '--context=prod get pods --all-namespaces --request-timeout=750ms -o json')
   printf '%s\n' '{"items":[
     {"metadata":{"namespace":"team-a","name":"web"},"status":{"phase":"Running","podIP":"192.0.2.42"},"spec":{"nodeName":"worker-a"}},
     {"metadata":{"namespace":"team-b","name":"web"},"status":{"phase":"Succeeded"}},
     {"metadata":{"namespace":"-bad","name":"web"},"status":{"phase":"Running"}}
   ]}' ;;
 '--context=prod --namespace=team-a get pod web --request-timeout=600ms -o json')
   printf '%s\n' '{"spec":{"containers":[{"name":"app","image":"app:stable"}],"initContainers":[{"name":"init","image":"init:stable"}]}}' ;;
 '--context=prod --namespace=team-a get deployments --request-timeout=300ms -o json')
   printf '%s\n' '{"items":[{"metadata":{"namespace":"team-a","name":"web-deploy"}}]}' ;;
 '--context=prod --namespace=team-a exec web -it -- printf ok') printf 'routed\n' ;;
 *) exit 90 ;;
esac
"#);
    assert!(e.live("pr", &["kube"]).contains("prod\tKubernetes context"));
    assert!(e.live("team", &["kube", "prod"]).contains("team-a\tKubernetes namespace · Active"));
    assert!(e.live("@", &["kube", "prod"]).starts_with("@all\t"));
    let all = e.live("team", &["kube", "prod", "@all", "logs"]);
    assert!(all.contains("team-a/web\tpods · Running · worker-a · 192.0.2.42") && all.contains("team-b/web"));
    assert!(!e.live("", &["kube", "prod", "@all", "exec"]).contains("team-b/web"));
    assert!(e.live("", &["kube", "prod", "@all", "get", "secrets"]).is_empty());
    assert!(e.live("dep", &["kube", "prod", "@all", "get"]).contains("deployments\t"));
    assert!(e.live("log", &["kube", "prod", "@all"]).contains("logs\t"));
    assert!(e.live("web", &["kube", "prod", "team-a", "get", "deploy"]).contains("web-deploy\t"));
    let containers = e.live("--container=", &["kube", "prod", "@all", "logs", "team-a/web"]);
    assert!(containers.contains("--container=app\tcontainer · app:stable") && containers.contains("--container=init\tinit container"));
    assert!(!e.live("", &["kube", "prod", "team-a", "exec", "web", "-c"]).contains("init"));
    let routed = e.cmd("complete-me").args(["kube", "prod", "@all", "exec", "team-a/web", "-it", "--", "printf", "ok"]).output().unwrap();
    assert!(routed.status.success());
    assert_eq!(routed.stdout, b"routed\n");
    let before = std::fs::read(e.tmp.join("kubectl.calls")).unwrap();
    for args in [
        vec!["kube","prod","@all","logs","team-a/web","--namespace=elsewhere"],
        vec!["kube","prod","@all","exec","team-a/web","-itnswitch"],
        vec!["kube","prod","@all","get","secret","team-a/my-secret"],
        vec!["kube","prod","@all","logs","web"],
        vec!["kube","--prod","team-a","logs","web"],
    ] {
        assert_eq!(e.cmd("complete-me").args(args).output().unwrap().status.code(), Some(2));
    }
    assert_eq!(std::fs::read(e.tmp.join("kubectl.calls")).unwrap(), before);
    let offline = e.cmd("complete-me").args(["live", "--explain", "--json", "--line", "kube prod @all logs "])
        .env("COMPLETE_ME_NO_REMOTE", "1").output().unwrap();
    assert_eq!(serde_json::from_slice::<serde_json::Value>(&offline.stdout).unwrap()["status"], "remote-disabled");
    if have("fish") {
        assert!(e.fish("kube prod @all logs team-a/").contains(&"team-a/web".into()));
    }
}

#[test]
fn aws_scopes_read_only_inventory_to_service_profile_and_region() {
    let e = Env::new("aws-scope");
    e.executable("aws", r#"
printf '%s\n' "$*" >> aws.calls
case "$*" in
 'configure list-profiles --no-cli-pager --no-cli-auto-prompt') printf 'dev\nwork\n' ;;
 '--profile=work --region=us-east-2 ec2 describe-instances --page-size 25 --max-items 25 '*)
   printf '%s\n' '{"Reservations":[{"Instances":[{"InstanceId":"i-123","State":{"Name":"running"},"InstanceType":"m7i.large","PrivateIpAddress":"192.0.2.11","Tags":[{"Key":"Name","Value":"build"}]}]}]}' ;;
 'eks list-clusters --page-size 50 --max-items 50 '*) printf '%s\n' '{"clusters":["workload"]}' ;;
 'ecs list-clusters --page-size 50 --max-items 50 '*) printf '%s\n' '{"clusterArns":["arn:aws:ecs:us-east-2:123:cluster/test"]}' ;;
 'lambda list-functions --max-items 50 '*) printf '%s\n' '{"Functions":[{"FunctionName":"convert","Runtime":"provided.al2023"}]}' ;;
 'rds describe-db-instances --page-size 25 --max-items 25 '*) printf '%s\n' '{"DBInstances":[{"DBInstanceIdentifier":"database","Engine":"postgres"}]}' ;;
 's3api list-buckets --page-size 50 --max-items 50 '*) printf '%s\n' '{"Buckets":[{"Name":"example-bucket","BucketRegion":"us-east-2"}]}' ;;
 *'ec2 describe-regions --all-regions '*) printf '%s\n' '{"Regions":[{"RegionName":"us-east-2","OptInStatus":"opt-in-not-required"}]}' ;;
 *) exit 87 ;;
esac
"#);
    assert!(e.live("--profile=wo", &["aws"]).contains("--profile=work\t"));
    assert!(e.live("--region=us", &["aws", "--profile", "work"]).contains("--region=us-east-2\t"));
    assert!(e.live("i-", &["aws","--profile","work","ec2","stop-instances","--region=us-east-2","--instance-ids"])
        .contains("i-123\tbuild · running · m7i.large · 192.0.2.11"));
    assert!(e.live("--function-name=co", &["aws","lambda","invoke"]).contains("--function-name=convert\tprovided.al2023"));
    assert!(e.live("work", &["aws","eks","update-kubeconfig","--name"]).contains("workload\t"));
    assert!(e.live("arn:", &["aws","ecs","list-services","--cluster"]).contains("cluster/test"));
    assert!(e.live("", &["aws","rds","start-db-instance","--db-instance-identifier"]).contains("database\tpostgres"));
    assert!(e.live("s3://exam", &["aws","s3","ls"]).contains("s3://example-bucket/\tus-east-2"));
    let before = std::fs::read(e.tmp.join("aws.calls")).unwrap();
    assert!(e.live("", &["aws","lambda","invoke","--endpoint-url=https://example.invalid","--function-name"]).is_empty());
    let offline = e.cmd("complete-me").args(["live", "--explain", "--json", "--line", "aws lambda invoke --function-name "])
        .env("COMPLETE_ME_NO_REMOTE", "1").output().unwrap();
    assert_eq!(serde_json::from_slice::<serde_json::Value>(&offline.stdout).unwrap()["status"], "remote-disabled");
    let scoped = e.cmd("complete-me").args(["live", "--explain", "--json", "--line", "AWS_SECRET_ACCESS_KEY=sentinel aws lambda invoke --function-name "]).output().unwrap();
    let report = String::from_utf8(scoped.stdout).unwrap();
    assert!(report.contains("scope-blocked") && !report.contains("sentinel"));
    assert_eq!(std::fs::read(e.tmp.join("aws.calls")).unwrap(), before);
}

#[test]
fn cloud_inventory_keeps_credentials_and_sensitive_dns_values_out_of_results() {
    let e = Env::new("cloud");
    e.executable("curl", r#"
printf '%s\n' "$@" >> curl.calls
input=$(cat)
case "$input" in
 *'Bearer MALFORMED'*) printf '%s\n' 'not JSON' ;;
 *'https://cloud.lambda.com/api/v1/instances?page_size=50'*'Bearer FIXTURE-LAMBDA'*)
   printf '%s\n' '{"data":[{"id":"a010-b011","name":"gpu","status":"active","region":{"name":"us-west"},"ip":"192.0.2.10"}]}' ;;
 *'https://api.dreamhost.com/'*'key=FIXTURE-DREAMHOST'*'cmd=dns-list_records'*)
   printf '%s\n' '{"result":"success","data":[{"record":"_acme.example.test","type":"TXT","value":"private-sentinel"},{"record":"www.example.test","type":"A","value":"192.0.2.4"}]}' ;;
 *) exit 88 ;;
esac
"#);
    for (line, variable, key, expected) in [
        ("cm-cloud lambda instances ", "COMPLETE_ME_LAMBDA_API_KEY", "FIXTURE-LAMBDA", "a010-b011\tgpu · active · us-west"),
        ("complete-me cloud dreamhost records ", "COMPLETE_ME_DREAMHOST_API_KEY", "FIXTURE-DREAMHOST", "_acme.example.test\tTXT"),
    ] {
        let output = e.cmd("complete-me").args(["live", "--line", line]).env(variable, key).output().unwrap();
        let output = String::from_utf8(output.stdout).unwrap();
        assert!(output.contains(expected), "{output:?}");
        assert!(!output.contains(key) && !output.contains("private-sentinel"));
        let report = e.cmd("complete-me").args(["live", "--explain", "--json", "--line", line]).env(variable, key).output().unwrap();
        assert!(!String::from_utf8(report.stdout).unwrap().contains(key));
    }
    assert!(e.live("lam", &["cm-cloud"]).contains("lambda\t"));
    assert!(e.live("inst", &["cm-cloud", "lambda"]).contains("instances\t"));
    assert!(e.live("rec", &["cm-cloud", "dreamhost"]).contains("records\t"));
    let result = e.cmd("complete-me").args(["cloud", "lambda", "instances", "a010-b011"])
        .env("COMPLETE_ME_LAMBDA_API_KEY", "FIXTURE-LAMBDA").output().unwrap();
    assert!(result.status.success() && String::from_utf8(result.stdout).unwrap().contains("a010-b011\t"));
    assert_eq!(e.cmd("complete-me").args(["cloud", "lambda", "launch"]).output().unwrap().status.code(), Some(2));
    let failure = e.cmd("complete-me").args(["live", "--explain", "--json", "--line", "cm-cloud lambda instances "])
        .env("COMPLETE_ME_LAMBDA_API_KEY", "MALFORMED").output().unwrap();
    assert_eq!(serde_json::from_slice::<serde_json::Value>(&failure.stdout).unwrap()["status"], "lookup-failed");
    let argv = std::fs::read_to_string(e.tmp.join("curl.calls")).unwrap();
    let missing = e.cmd("complete-me").args(["live", "--explain", "--json", "--line", "cm-cloud lambda instances "])
        .env("COMPLETE_ME_LAMBDA_API_KEY", "bad\nkey").output().unwrap();
    assert_eq!(serde_json::from_slice::<serde_json::Value>(&missing.stdout).unwrap()["status"], "credentials-unavailable");
    assert_eq!(std::fs::read_to_string(e.tmp.join("curl.calls")).unwrap(), argv);
    assert!(argv.starts_with("--disable\n--config\n-\n--proto\n=https\n") && !argv.contains("FIXTURE"));
    let blocked = e.cmd("complete-me").args(["live", "--explain", "--json", "--line", "cm-cloud dreamhost records "])
        .env("COMPLETE_ME_DREAMHOST_API_KEY", "FIXTURE-DREAMHOST").env("COMPLETE_ME_NO_REMOTE", "1").output().unwrap();
    assert_eq!(serde_json::from_slice::<serde_json::Value>(&blocked.stdout).unwrap()["status"], "remote-disabled");
    assert_eq!(std::fs::read_to_string(e.tmp.join("curl.calls")).unwrap(), argv);
}

#[test]
fn vultr_and_hugging_face_selection_respects_scope_and_forwards_only_reads() {
    let e = Env::new("vultr-hf");
    e.executable("vultr-cli", r#"
case "$*" in
 '--config=lab.yaml instance list --per-page 50 --output json')
   printf '%s\n' '{"instances":[{"id":"a-1","label":"bench","region":"ewr","status":"active","main_ip":"192.0.2.11"}]}' ;;
 'regions list --output json') printf '%s\n' '[{"id":"ewr","city":"Newark","country":"US"}]' ;;
 'dns domain list --output json') printf '%s\n' '{"domains":[{"domain":"example.test","date_created":"2026-01-01"}]}' ;;
 'kubernetes list --output json') printf '%s\n' '{"vke_clusters":[{"id":"cluster-a","label":"bench"}]}' ;;
 *) exit 86 ;;
esac
"#);
    e.executable("hf", r#"
[ "$HF_HUB_OFFLINE:$HF_HUB_DISABLE_TELEMETRY" = "1:1" ] || exit 84
case "$*" in
 'cache ls --format json'|'cache ls --format json --cache-dir custom')
   printf '%s\n' '[{"id":"model/asset/repo","size_on_disk":100},{"repo_type":"dataset","repo_id":"asset/sample"}]' ;;
 *) exit 85 ;;
esac
"#);
    assert!(e.live("a-", &["vultr-cli", "--config", "lab.yaml", "instance", "delete"]).contains("a-1\tbench · active · ewr"));
    assert!(e.live("--region=ew", &["vultr-cli", "instance", "create"]).contains("--region=ewr\tNewark"));
    assert!(e.live("", &["vultr-cli", "dns", "domain", "get"]).contains("example.test\t"));
    assert!(e.live("", &["vultr-cli", "kubernetes", "get"]).contains("cluster-a\t"));
    let offline = e.cmd("complete-me").args(["live","--line","hf cache rm "]).env("COMPLETE_ME_NO_REMOTE", "1").output().unwrap();
    assert!(String::from_utf8(offline.stdout).unwrap().contains("model/asset/repo\tmodel"));
    assert!(e.live("asset/", &["hf","download","--repo-type=dataset","--cache-dir","custom"]).contains("asset/sample\tdataset"));
    assert!(!e.live("", &["hf","download"]).contains("asset/sample"));
}

#[test]
fn agent_catalog_and_session_lookup_never_launch_agents_or_read_titles() {
    let e = Env::new("agents");
    let config = e.tmp.join("config/complete-me");
    std::fs::create_dir_all(&config).unwrap();
    std::fs::write(config.join("agents.json"), r#"{
        "codex":{"models":{"lab/model":"Local model"},"values":{"--profile":{"lab":"Workspace profile"}}},
        "opencode":{"sessions":{"ses_fixture":"Saved project session"}},
        "fx":{"commands":{"models":"Local custom command"},"values":{"--preset":{"analysis":"Configured preset"}}},
        "custom-agent":{"commands":{"chat":"Open a session"}}
    }"#).unwrap();
    let sessions = e.tmp.join("codex-home");
    std::fs::create_dir(&sessions).unwrap();
    let id = "42000000-1234-4abc-9234-000000000001";
    std::fs::write(sessions.join("session_index.jsonl"), format!(
        "{{\"id\":\"{id}\",\"thread_name\":\"private title\",\"updated_at\":\"2026-04-01T11:12:13Z\"}}\n{{\"id\":\"not-a-uuid\"}}\n"
    )).unwrap();
    for tool in ["codex","claude","gemini","agy","hermes","fx","cline","opencode","aider","cursor-agent","copilot","droid","qwen","goose","amp","pi","custom-agent"] {
        e.executable(tool, "touch agent-was-run; exit 70");
    }
    assert!(e.live("lab", &["codex","--model"]).contains("lab/model\tLocal model"));
    assert!(e.live("--profile=la", &["codex"]).contains("--profile=lab\tWorkspace profile"));
    assert!(e.live("plan", &["agy","--mode"]).contains("plan\tAgent option value"));
    assert!(e.live("lat", &["gemini","--resume"]).contains("latest\t"));
    assert!(e.live("so", &["claude","--model"]).contains("sonnet\t"));
    assert!(e.live("ses", &["opencode","--session"]).contains("ses_fixture\t"));
    assert!(e.live("ana", &["fx","--preset"]).contains("analysis\tConfigured preset"));
    assert!(e.live("cha", &["custom-agent"]).contains("chat\t"));
    assert!(e.live("lab", &["codex","--","--model"]).is_empty());
    let exact = e.cmd("complete-me").args(["live","--line","codex resume 42"])
        .env("CODEX_HOME", sessions).output().unwrap();
    let output = String::from_utf8(exact.stdout).unwrap();
    assert!(output.contains(id) && output.contains("2026-04-01") && !output.contains("private title"));
    let claude_home = e.tmp.join("claude-home");
    let cwd = e.tmp.canonicalize().unwrap().to_string_lossy().chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' }).collect::<String>();
    let project = claude_home.join("projects").join(cwd);
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(project.join(format!("{id}.jsonl")), "private transcript").unwrap();
    let claude = e.cmd("complete-me").args(["live","--line","claude --resume=42"])
        .env("CLAUDE_CONFIG_DIR", claude_home).output().unwrap();
    let claude = String::from_utf8(claude.stdout).unwrap();
    assert!(claude.contains(id) && !claude.contains("private transcript"), "{claude:?}");
    assert!(e.live("", &["codex","exec"]).contains("resume\t"));
    let gen = e.cmd("complete-me").args(["gen","--force","--quick","codex","custom-agent"]).output().unwrap();
    assert!(gen.status.success(), "{}", String::from_utf8_lossy(&gen.stderr));
    assert!(!e.tmp.join("agent-was-run").exists());
    if have("fish") {
        assert!(e.fish("codex --model la").contains(&"lab/model".into()));
    }
}

#[test]
fn proxmox_backup_and_restore_use_node_scoped_inventory() {
    let e = Env::new("pve-backup");
    let scripts = [
        ("pvesh", r#"
case "$*" in
 'get /storage --output-format json')
   printf '%s\n' '[{"storage":"vault","type":"pbs","content":"backup","nodes":"pve-a"},{"storage":"directory","type":"dir","content":"backup","path":"/configured/store","content-dirs":"backup=archives"},{"storage":"offline","type":"pbs","content":"backup","disable":1},{"storage":"other-node","type":"pbs","content":"backup","nodes":"pve-b"},{"storage":"cold","type":"nfs","content":"backup","path":"/mnt/pve/cold"},{"storage":"vm-pool","type":"lvmthin","content":"images"},{"storage":"ct-pool","type":"zfspool","content":"rootdir"}]' ;;
 'get /nodes/localhost/storage/vault/content --content backup --output-format json')
   printf '%s\n' '[{"volid":"vault:backup/vm/210/2025-10-05T10:20:30Z","ctime":2,"notes":"secret-note"},{"volid":"vault:backup/vm/210/2025-10-04T10:20:30Z","ctime":1},{"volid":"vault:backup/ct/211/2025-10-05T10:20:30Z"},{"volid":"other:backup/vm/210/2025-10-05T10:20:30Z"},{"volid":"vault:backup/../somewhere"}]' ;;
 'get /cluster/resources --type vm --output-format json')
   printf '%s\n' '[{"vmid":210,"name":"db","node":"pve-a","type":"qemu"},{"vmid":211,"name":"web","node":"pve-a","type":"lxc"},{"vmid":212,"name":"remote","node":"pve-b","type":"qemu"}]' ;;
 'get /cluster/nextid --output-format json') printf '%s\n' '"301"' ;;
 *) exit 91 ;;
esac
"#),
        ("perl", r#"
test "$1" = -e && test "$3" = -- || exit 90
case "$4:$5:$6:$7" in
  '/configured/store:archives::dir')
    printf '%s\0' '30 vzdump-lxc-211-2025_10_05-10_20_30.tar.zst' '29 vzdump-qemu-210-2025_10_05-10_20_30.vma.zst' '28 vzdump-qemu-210-2025_10_05-10_20_30.vma.zst.notes' ;;
  '/mnt/pve/cold:dump:/mnt/pve/cold:nfs') : > checked-cold-mount ;;
  *) exit 90 ;;
esac
"#),
        ("hostname", "test \"$*\" = -s && printf '%s\\n' \"${CM_TEST_OBSERVED_NODE:-pve-a}\""),
    ];
    for (program, script) in scripts {
        e.executable(program, script);
    }
    e.executable("qmrestore", "test \"${CM_TEST_SUBMITTED:-0}\" = 1 && : > restore-submitted");
    e.executable("pct", "exit 92");
    e.executable("pvesm", "touch pvesm-called; exit 92");
    let stores = e.live("", &["qmrestore"]);
    assert!(stores.contains("vault:\tbackup storage · pbs"));
    assert!(stores.contains("directory:\tbackup storage · dir"));
    assert!(!stores.contains("offline") && !stores.contains("other-node"));
    let rows = e.live("vault:", &["qmrestore"]);
    assert!(rows.starts_with("vault:backup/vm/210/2025-10-05T10:20:30Z\tGuest 210 · current guest name: db · 2025-10-05T10:20:30Z"));
    assert_eq!(rows.lines().count(), 2);
    assert!(!rows.contains("secret-note"));
    assert!(e.live("vault:", &["pct", "restore", "301"]).starts_with("vault:backup/ct/211/"));
    let rows = e.live("directory:", &["qmrestore"]);
    assert!(rows.starts_with("directory:backup/vzdump-qemu-210"));
    assert_eq!(rows.lines().count(), 1);
    assert!(e.live("directory:", &["pct", "restore", "301"]).starts_with("directory:backup/vzdump-lxc-211-"));
    assert!(e.live("cold:", &["qmrestore"]).is_empty());
    assert!(e.tmp.join("checked-cold-mount").is_file());
    assert!(!e.tmp.join("pvesm-called").exists());
    let local_offline = e.cmd("complete-me").env("COMPLETE_ME_NO_REMOTE", "1")
        .args(["live", "--explain", "--json", "--line", "qmrestore vault:"]).output().unwrap();
    assert_eq!(serde_json::from_slice::<serde_json::Value>(&local_offline.stdout).unwrap()["status"], "remote-disabled");
    assert_eq!(e.live("30", &["pct", "restore"]), "301\tNext unused guest ID at lookup time\n");
    assert_eq!(e.live("", &["qmrestore", "vault:backup/vm/210/2025-10-05T10:20:30Z", "301", "--storage"]), "vm-pool\timages storage · lvmthin\n");
    assert_eq!(e.live("--storage=ct", &["pct", "restore", "301", "vault:backup/vzdump-lxc-211-2025_10_05-10_20_30.tar.zst"]), "--storage=ct-pool\trootdir storage · zfspool\n");
    assert!(e.live("", &["vzdump"]).contains("210\tdb"));
    assert!(e.live("", &["vzdump"]).contains("211\tweb"));
    assert!(!e.live("", &["vzdump"]).contains("212\t"));
    assert!(!e.live("", &["vzdump", "210"]).contains("210\t"));
    assert!(e.live("", &["vzdump", "--all"]).is_empty());
    assert!(e.live("", &["vzdump", "--node", "pve-b"]).is_empty());

    std::fs::create_dir_all(e.tmp.join("config/complete-me")).unwrap();
    std::fs::write(e.tmp.join("config/complete-me/proxmox-hosts"), "node-a-ssh\n").unwrap();
    std::fs::write(e.tmp.join("config/complete-me/proxmox-clusters"), "lab node-a-ssh pve-a=node-a-ssh\n").unwrap();
    e.executable("ssh", r#"
printf '%s\n' "$@" >> ssh-backup.calls
for last do :; done
case "$last" in
 "'sh' '-c' "*" 'complete-me' 'pve-a' 'pvesh' 'get' '/storage' '--output-format' 'json'"|"'sh' '-c' "*" 'complete-me' 'pve-a' 'hostname' '-s'")
   /bin/sh -c "$last" ;;
 "'sh' '-c' "*" 'complete-me' 'pve-a' 'qmrestore' 'vault:backup/vm/210/2025-10-05T10:20:30Z' '301' '--storage' 'vm-pool'")
   test "$1" = -T && CM_TEST_SUBMITTED=1 /bin/sh -c "$last" ;;
 *) exit 93 ;;
esac
"#);
    assert_eq!(e.live("pv", &["proxmox", "lab", "--node"]), "pve-a\tSSH via node-a-ssh\n");
    assert_eq!(e.live("vault", &["proxmox", "lab", "--node", "pve-a", "qmrestore"]), "vault:\tbackup storage · pbs\n");
    let calls = std::fs::read_to_string(e.tmp.join("ssh-backup.calls")).unwrap();
    assert!(calls.contains("StrictHostKeyChecking=yes") && calls.contains("\nnode-a-ssh\n"));
    assert!(!calls.contains("qmrestore"));
    let blocked = e.cmd("complete-me").env("COMPLETE_ME_NO_REMOTE", "1")
        .args(["live", "--line", "proxmox lab --node pve-a qmrestore "]).output().unwrap();
    assert!(blocked.stdout.is_empty());
    assert_eq!(std::fs::read_to_string(e.tmp.join("ssh-backup.calls")).unwrap(), calls);
    let wrong_lookup = e.cmd("complete-me").env("CM_TEST_OBSERVED_NODE", "pve-b")
        .args(["live", "--line", "proxmox lab --node pve-a qmrestore "]).output().unwrap();
    assert!(wrong_lookup.stdout.is_empty());
    let restore = ["proxmox", "lab", "--node", "pve-a", "qmrestore", "vault:backup/vm/210/2025-10-05T10:20:30Z", "301", "--storage", "vm-pool"];
    let different_node = e.cmd("complete-me").args(restore).env("CM_TEST_OBSERVED_NODE", "pve-b").output().unwrap();
    assert_eq!(different_node.status.code(), Some(125));
    assert!(!e.tmp.join("restore-submitted").exists());
    assert!(e.cmd("complete-me").args(restore).status().unwrap().success());
    assert!(e.tmp.join("restore-submitted").is_file());
}

#[cfg(target_os = "linux")]
#[test]
fn proxmox_directory_inventory_reads_the_existing_directory_without_following_symlinks() {
    let e = Env::new("pve-directory");
    let dump = e.tmp.join("storage/dump");
    std::fs::create_dir_all(&dump).unwrap();
    let archive = "vzdump-qemu-210-2025_10_05-10_20_30.vma.zst";
    for filename in [
        archive.to_owned(),
        format!("{archive}.notes"),
        "vzdump-qemu-210-2025_10_05-10_20_30.vma.".into(),
    ] {
        std::fs::write(dump.join(filename), "").unwrap();
    }
    std::os::unix::fs::symlink("storage", e.tmp.join("linked")).unwrap();
    std::os::unix::fs::symlink(archive, dump.join("vzdump-qemu-211-2025_10_05-10_20_31.vma.zst")).unwrap();
    e.executable("pvesh", r#"
case "$*" in
 'get /storage --output-format json')
    printf '[{"storage":"local","type":"dir","content":"backup","path":"%s/storage"},{"storage":"alias","type":"dir","content":"backup","path":"%s/linked"}]\n' "$PWD" "$PWD" ;;
 'get /cluster/resources --type vm --output-format json') printf '[]\n' ;;
 *) exit 93 ;;
esac
"#);
    let rows = e.live("local:", &["qmrestore"]);
    assert_eq!(rows.lines().count(), 1, "{rows}");
    assert!(rows.starts_with(&format!("local:backup/{archive}\t")));
    assert!(e.live("alias:", &["qmrestore"]).is_empty());
}

#[test]
fn s3_object_and_prefix_completions_keep_scope_and_editor_suffix() {
    let e = Env::new("s3-object");
    e.executable("aws", r#"
printf '%s\n' "$@" >> s3.calls
case "$*" in
  *'s3api list-objects-v2 --no-paginate --cli-input-json '*)
  printf '%s\n' '{"CommonPrefixes":[{"Prefix":"logs/daily/"},{"Prefix":"wrong/prefix/"}],"Contents":[{"Key":"logs/data file.txt","Size":42,"LastModified":"2025-10-05T00:00:00Z"},{"Key":"logs/line\nbreak"},{"Key":"wrong/sibling.txt"},{"Key":"file://local.txt"}]}' ;;
 *) exit 94 ;;
esac
"#);
    let rows = e.live("s3://example-bucket/logs/d", &["aws", "--profile", "test", "s3", "cp", "--region=us-west-2"]);
    assert!(rows.contains("s3://example-bucket/logs/daily/\tS3 prefix"));
    assert!(rows.contains("s3://example-bucket/logs/data file.txt\tS3 object · 42 B · 2025-10-05"));
    assert!(!rows.contains("wrong") && !rows.contains("break"));
    let rows = e.live("--key=logs", &["aws", "s3api", "get-object", "--bucket", "example-bucket", "--expected-bucket-owner=123456789012"]);
    assert!(rows.contains("--key=logs/data file.txt\tS3 object"));
    assert!(e.live("logs/", &["aws", "s3api", "list-objects-v2", "--bucket=example-bucket", "--prefix"]).contains("logs/daily/\tS3 prefix"));
    assert!(!e.live("", &["aws", "s3api", "get-object", "--bucket", "example-bucket", "--key"]).contains("file://"));
    let calls = std::fs::read_to_string(e.tmp.join("s3.calls")).unwrap();
    assert!(calls.contains("--profile=test\n--region=us-west-2\ns3api\nlist-objects-v2\n--no-paginate"));
    assert!(calls.contains(r#""MaxKeys":50"#) && calls.contains(r#""Prefix":"logs/d""#));
    assert!(calls.contains(r#""ExpectedBucketOwner":"123456789012""#));
    for words in [
        vec!["aws", "s3", "ls", "--endpoint-url=https://wrong.example"],
        vec!["aws", "s3", "ls", "--endp=https://wrong.example"],
        vec!["aws", "s3", "ls", "--request-payer", "requester"],
    ] {
        assert!(e.live("s3://example-bucket/logs/", &words).is_empty());
    }
    assert!(e.live("s3://127.0.0.1/logs/", &["aws","s3","ls"]).is_empty());
    let offline = e.cmd("complete-me").env("COMPLETE_ME_NO_REMOTE", "1")
        .args(["live", "--line", "aws s3 ls s3://example-bucket/logs/"]).output().unwrap();
    assert!(offline.stdout.is_empty());
    assert_eq!(std::fs::read_to_string(e.tmp.join("s3.calls")).unwrap(), calls);
    for shell in ["bash", "zsh", "fish"] {
        if !have(shell) || (shell != "zsh" && !have("python3")) { continue; }
        let before = std::fs::read_to_string(e.tmp.join("s3.calls")).unwrap();
        let buffer = e.edit_with_tab(shell, "aws s3 cp s3://example-bucket/logs/dat", " ./download --profile right-profile");
        let words = complete_me::extras::split_line(&buffer);
        assert_eq!(words.words, ["aws", "s3", "cp", "s3://example-bucket/logs/data file.txt", "./download", "--profile"], "{shell}: {buffer:?}");
        assert_eq!(words.cur, "right-profile");
        let editor_calls = std::fs::read_to_string(e.tmp.join("s3.calls")).unwrap();
        assert!(editor_calls.strip_prefix(&before).unwrap().contains("--profile=right-profile\ns3api\nlist-objects-v2"), "{shell}");
        let orig = "aws s3 cp s3://example-bucket/logs/dat";
        let tail = " ./download --endpoint-url=https://alternate.example";
        assert_eq!(e.edit_with_tab(shell, orig, tail), format!("{orig}{tail}"), "{shell}");
        assert_eq!(std::fs::read_to_string(e.tmp.join("s3.calls")).unwrap(), editor_calls, "{shell}");
    }
    if have("fish") && have("python3") {
        let runtime = Path::new(env!("CARGO_MANIFEST_DIR")).join("shell/fish/complete_me.fish");
        let before = std::fs::read_to_string(e.tmp.join("s3.calls")).unwrap();
        let out = e.cmd("python3").env("HOME", &e.tmp).args([
            &harness("cursor.py"), "fish", runtime.to_str().unwrap(),
            "aws s3 cp s3://example-bucket/logs/dat", " ./download --profile native-profile", "native",
        ]).output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        let calls = std::fs::read_to_string(e.tmp.join("s3.calls")).unwrap();
        assert!(!calls.strip_prefix(&before).unwrap().contains("list-objects-v2"));
    }
    let rows = String::from_utf8(e.cmd("complete-me").args([
        "live", "--line", "aws s3api get-object --key logs/d", "--right", " --bucket example-bucket --region us-east-2 ./download",
    ]).output().unwrap().stdout).unwrap();
    assert!(rows.contains("logs/data file.txt"));
    let before = std::fs::read(e.tmp.join("s3.calls")).unwrap();
    let line = "aws --profile alternate s3 ls s3://example-bucket/logs/";
    std::fs::write(e.tmp.join("aws-config"), "[profile alternate]\nendpoint_url = https://alternate.example\n").unwrap();
    let configured = e.cmd("complete-me").args(["live", "--line", line, "--explain", "--json"]).output().unwrap();
    assert_eq!(serde_json::from_slice::<serde_json::Value>(&configured.stdout).unwrap()["sources"][0]["reason"], "custom-endpoint-or-config");
    let endpoint = e.cmd("complete-me").env("AWS_ENDPOINT_URL_S3", "https://alternate.example")
        .args(["live", "--line", "aws s3 ls s3://example-bucket/logs/"]).output().unwrap();
    assert!(endpoint.stdout.is_empty());
    assert!(e.live("s3://example-bucket/logs/", &["env", "AWS_CONFIG_FILE=./project.ini", "aws", "s3", "ls"]).is_empty());
    assert_eq!(std::fs::read(e.tmp.join("s3.calls")).unwrap(), before);
}
