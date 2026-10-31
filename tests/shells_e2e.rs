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
        Env { tmp, path }
    }
    fn cmd(&self, prog: &str) -> Command {
        let mut c = Command::new(prog);
        c.current_dir(&self.tmp)
            .env("PATH", &self.path)
            .env("XDG_CACHE_HOME", self.tmp.join("cache"))
            .env("XDG_CONFIG_HOME", self.tmp.join("config"))
            .env("XDG_DATA_HOME", self.tmp.join("data"))
            .env("complete_me_force", "1");
        c
    }
    fn fish_with(&self, line: &str, clip: Option<&str>) -> Vec<(String, String)> {
        let rt = Path::new(env!("CARGO_MANIFEST_DIR")).join("shell/fish/complete_me.fish");
        let script = format!("set -g fish_history fish; source {}; function cargo-like; end; complete -C {}", rt.display(), fq(line));
        let mut c = self.cmd("fish");
        c.args(["--no-config", "-c", &script]);
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
    // native completions stay intact underneath
    if has_bash_completion() {
        assert!(e.bash("ls --colo", &[], None, true).iter().any(|w| w.starts_with("--color")));
    }
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
