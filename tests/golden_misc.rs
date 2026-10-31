//! ssh_config, bash→fish, snippets and aliases must reproduce the Python goldens exactly.
use complete_me::{aliases, bashfish, snippets, sshopts};

fn golden(name: &str) -> String {
    std::fs::read_to_string(format!("tests/golden/{name}")).unwrap()
}

fn compare(name: &str, got: &str, fails: &mut Vec<String>) {
    let want = golden(name);
    if want == got {
        return;
    }
    let (w, g): (Vec<&str>, Vec<&str>) = (want.lines().collect(), got.lines().collect());
    for i in 0..w.len().max(g.len()) {
        let (a, b) = (w.get(i).copied().unwrap_or("<EOF>"), g.get(i).copied().unwrap_or("<EOF>"));
        if a != b {
            fails.push(format!("{name} line {}:\n  want: {a}\n  got : {b}", i + 1));
            return;
        }
    }
    fails.push(format!("{name}: trailing whitespace differs"));
}

#[test]
fn misc_goldens() {
    let mut fails = vec![];
    let keys = sshopts::parse_ssh_config_man(&std::fs::read_to_string("tests/fixtures/ssh_config.man").unwrap());
    let got: String = keys.iter().map(|(k, (d, v))| format!("{k}\t{d}\t{}\n", v.join(","))).collect();
    compare("ssh.keys.txt", &got, &mut fails);

    let samples = ["curl -fsSL https://x | bash", "for f in *; do rm -rf \"$f\"; done", "sudo systemctl restart x",
        "for i in 1 2; do echo \"$i\"; done", "echo sudoku; echo formula", "dd if=/dev/zero of=/dev/sda",
        "git push --force origin main", "echo hi > ~/.bashrc", ":(){ :|:& };:", "eval $x", "rm /tmp/x",
        "chmod -R 777 /etc", "for i in {1..3}; do :; done", "echo {5..1} {1..10..2} '{1..3}' a{1..2}"];
    let got: Vec<String> = samples.iter().map(|s| format!("{s}\t{}", bashfish::risks(s).join("|"))).collect();
    compare("bashfish.txt", &(got.join("\n") + "\n"), &mut fails);

    compare("snippets.abbrs.fish", &(snippets::fish_abbrs() + "\n"), &mut fails);
    compare("snippets.list.txt", &(snippets::list("fish") + "\n"), &mut fails);
    compare("snippets.phrasebook.txt", &(snippets::phrasebook() + "\n"), &mut fails);

    let bodies = ["git switch", "journalctl -u $1 -f", "ls -la", "mkdir -p $1; and cd $1", "echo $@ done", "cmd $argv[2] x",
        "git commit -m \"$1\"", "command ls"];
    let got: Vec<String> = bodies.iter().map(|b| format!("{b}\t{}\t{}\t{}", aliases::to_fish(b), aliases::to_sh(b), aliases::wraps_of(b))).collect();
    compare("aliases.txt", &(got.join("\n") + "\n"), &mut fails);

    let dir = std::env::temp_dir().join(format!("cm-alias-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::env::set_var("XDG_CONFIG_HOME", &dir);
    for (name, body) in [("gsw", "git switch"), ("logs", "journalctl -u $1 -f"), ("ls", "ls -la")] {
        let p = aliases::write_fish(name, body, &format!("alias {name}={body}"), true).unwrap();
        compare(&format!("alias.{name}.fish"), &std::fs::read_to_string(p).unwrap(), &mut fails);
        aliases::write_sh("bash", name, body, true);
    }
    compare("alias.bash", &std::fs::read_to_string(dir.join("complete-me/aliases.bash")).unwrap(), &mut fails);
    // a file the user wrote is never overwritten
    std::fs::write(dir.join("fish/functions/mine.fish"), "function mine; end\n").unwrap();
    assert!(aliases::write_fish("mine", "echo hi", "x", true).is_err());
    let _ = std::fs::remove_dir_all(&dir);
    assert!(fails.is_empty(), "\n{}", fails.join("\n\n"));
}

#[test]
fn stdin_child_is_killed_on_timeout() {
    let t = std::time::Instant::now();
    assert!(bashfish::run_stdin(&["sh", "-c", "sleep 10"], "", std::time::Duration::from_millis(300)).is_none());
    assert!(t.elapsed() < std::time::Duration::from_secs(2));
}
