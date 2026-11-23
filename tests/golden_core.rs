//! The parser, script reader and shell renderers are checked against committed snapshots.
use complete_me::{parse, render, scriptscan};
use std::path::Path;

const FIXTURES: &[&str] = &["cargo", "claude", "claude-mcp", "docker", "git", "ls", "pip", "rg", "zerotier-cli"];

fn golden(name: &str) -> String {
    std::fs::read_to_string(Path::new("tests/golden").join(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn norm(s: &str) -> String {
    s.replace(&format!("complete-me {}", complete_me::VERSION), "complete-me VERSION")
}

/// Returns a description of the first difference, or None if equal.
fn diff(want: &str, got: &str) -> Option<String> {
    if want == got {
        return None;
    }
    let (w, g): (Vec<&str>, Vec<&str>) = (want.lines().collect(), got.lines().collect());
    for i in 0..w.len().max(g.len()) {
        let (a, b) = (w.get(i).copied().unwrap_or("<EOF>"), g.get(i).copied().unwrap_or("<EOF>"));
        if a != b {
            return Some(format!("line {}:\n  want: {a}\n  got : {b}", i + 1));
        }
    }
    Some("trailing newline differs".into())
}

fn check(results: &mut Vec<String>, label: &str, want: &str, got: &str) {
    if let Some(d) = diff(want, got) {
        results.push(format!("{label}: {d}"));
    }
}

fn check_render(results: &mut Vec<String>, name: &str, result: &str) {
    let result = norm(result);
    if std::env::var_os("COMPLETE_ME_UPDATE_RENDER_GOLDENS").is_some() {
        std::fs::write(Path::new("tests/golden").join(name), &result).unwrap();
    }
    check(results, name, &golden(name), &result);
}

#[test]
fn core_goldens() {
    let mut failures = vec![];
    let mut total = 0;
    for f in FIXTURES {
        let text = std::fs::read_to_string(format!("tests/fixtures/{f}.help")).unwrap();
        let c = parse::parse_help(&text, f, "help");
        total += 3;
        check(&mut failures, &format!("parse.{f}"), &golden(&format!("parse.{f}.txt")), &c.dump());
        check_render(&mut failures, &format!("fish.{f}.fish"), &render::fish(&c).unwrap());
        check_render(&mut failures, &format!("bash.{f}.bash"), &render::bash(&c).unwrap());
    }
    for s in ["tool.sh", "tool.py"] {
        total += 1;
        let c = scriptscan::scan(Path::new(&format!("tests/fixtures/scripts/{s}")), "tool").expect(s);
        check(&mut failures, &format!("script.{s}"), &golden(&format!("script.{s}.txt")), &c.dump());
    }
    eprintln!("core goldens: {}/{} match", total - failures.len(), total);
    assert!(failures.is_empty(), "\n{}", failures.join("\n\n"));
}

#[test]
fn hostile_names_never_reach_the_shell() {
    let mut c = complete_me::model::Command::with_source("evil", "help");
    let mut ok = complete_me::model::Command::new("ok");
    ok.desc = "it's $(rm -rf ~) `boom` \\ done".into();
    c.subcommands.insert("ok".into(), ok);
    c.subcommands.insert("bad;rm".into(), complete_me::model::Command::new("bad;rm"));
    for out in [render::fish(&c).unwrap(), render::bash(&c).unwrap(), render::zsh(&c).unwrap()] {
        assert!(!out.contains("bad;rm"), "{out}");
    }
    assert!(render::fish(&c).unwrap().contains("'it\\'s $(rm -rf ~) `boom` \\\\ done'"));
    assert!(render::fish(&complete_me::model::Command::new("../../x")).is_err());
}
