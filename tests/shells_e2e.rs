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
