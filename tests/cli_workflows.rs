//! CLI behavior isolated from the user's config, cache, and installed completions.
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

pub(crate) struct Fixture {
    dir: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let name = std::thread::current().name().unwrap_or("fixture").to_string();
        let dir = std::env::temp_dir().join(format!("cm-cli-{name}-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        Self { dir }
    }

    fn command(&self) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_complete-me"));
        cmd.current_dir(&self.dir)
            .env("HOME", &self.dir)
            .env("XDG_CONFIG_HOME", self.dir.join("config"))
            .env("XDG_CACHE_HOME", self.dir.join("cache"))
            .env("COMPLETE_ME_SANDBOX", "off")
            .env(
                "PATH",
                format!("{}:{}", self.dir.join("bin").display(), std::env::var("PATH").unwrap()),
            );
        cmd
    }

    fn run(&self, args: &[&str]) -> Output {
        self.command().args(args).output().unwrap()
    }

    fn check(&self, args: &[&str], expected: i32) -> String {
        let output = self.run(args);
        assert_eq!(
            output.status.code(),
            Some(expected),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    }

    fn executable(&self, name: &str, contents: &str) {
        use std::os::unix::fs::PermissionsExt;
        let path = self.dir.join("bin").join(name);
        std::fs::write(&path, contents).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn cache(&self) -> PathBuf {
        self.dir.join("cache/complete-me")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn generated_cache_lifecycle_and_quick_refresh() {
    let f = Fixture::new();
    let tool = "fixture-cmd";
    f.executable(
        tool,
        &format!(
            "{}\nfrom pathlib import Path\nPath('ran-script').write_text('unexpected')\n",
            include_str!("fixtures/scripts/tool.py")
        ),
    );
    f.executable("man", "#!/bin/sh\nexit 1\n");
    assert!(f.check(&["gen", "--quick", tool], 0).contains("options"));
    assert!(!f.dir.join("ran-script").exists());
    let paths = [
        "fish/fixture-cmd.fish",
        "bash/fixture-cmd",
        "zsh/fixture-cmd.zsh",
        "json/fixture-cmd.json",
    ];
    assert!(paths.iter().all(|path| f.cache().join(path).is_file()));
    let fish = f.cache().join(paths[0]);
    assert!(std::fs::read_to_string(&fish).unwrap().contains("--quick pass"));
    assert!(f.check(&["list"], 0).lines().any(|s| s == tool));
    let json: serde_json::Value = serde_json::from_str(&f.check(&["show", "--quick", "--json", tool], 0)).unwrap();
    assert!(json["subcommands"]["run"].is_object());
    assert!(f.check(&["show", tool], 0).contains("fixture-cmd run"));
    assert!(f
        .check(&["explain", "--", tool, "run", "--dry-run"], 0)
        .contains("subcommand"));
    f.check(&["gen", tool], 0);
    assert!(!std::fs::read_to_string(&fish).unwrap().contains("--quick pass"));
    assert_eq!(f.check(&["gen", tool], 0), "");
    f.check(&["forget", tool], 0);
    assert!(paths.iter().all(|path| !f.cache().join(path).exists()));
    assert!(f.check(&["list"], 0).is_empty());
}

#[test]
fn failed_cache_writes_can_be_retried_and_misses_can_be_refreshed() {
    let f = Fixture::new();
    f.executable("fixture-cache", include_str!("fixtures/scripts/tool.sh"));
    f.executable("man", "#!/bin/sh\nexit 1\n");
    let obstruction = f.cache().join("bash/fixture-cache");
    std::fs::create_dir_all(&obstruction).unwrap();
    let first = f.run(&["gen", "fixture-cache"]);
    assert_eq!(first.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&first.stderr).contains("could not write the cache"));
    assert!(f.cache().join("fish/fixture-cache.fish").is_file());
    std::fs::remove_dir(&obstruction).unwrap();
    f.check(&["gen", "fixture-cache"], 0);
    assert!(obstruction.is_file() && f.cache().join("json/fixture-cache.json").is_file());

    f.check(&["gen", "--quiet", "fixture-miss"], 1);
    assert!(f.cache().join("miss/fixture-miss").is_file());
    f.executable("fixture-miss", include_str!("fixtures/scripts/tool.sh"));
    assert!(f.check(&["gen", "fixture-miss"], 0).is_empty());
    f.check(&["gen", "--force", "fixture-miss"], 0);
    assert!(!f.cache().join("miss/fixture-miss").exists());
}

#[test]
fn bad_arguments_and_unavailable_commands_return_expected_status() {
    let f = Fixture::new();
    for (args, status) in [
        (&[][..], 2),
        (&["--version"][..], 0),
        (&["--help"][..], 0),
        (&["unknown"][..], 2),
        (&["gen"][..], 2),
        (&["gen", "invalid/name"][..], 1),
        (&["show"][..], 2),
        (&["show", "fixture-unavailable"][..], 1),
        (&["forget"][..], 2),
        (&["doctor", "--shell", "unknown"][..], 2),
        (&["doctor", "--command", "bad name"][..], 2),
    ] {
        f.check(args, status);
    }
    let file = f.cache().join("bash/fixture-existing");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, "generated").unwrap();
    f.check(&["forget", "fixture-existing", "unsafe command name"], 2);
    assert!(file.is_file());
}

#[test]
fn doctor_standalone_inspects_disk_and_live_state() {
    let f = Fixture::new();
    f.executable(
        "systemctl",
        "#!/bin/sh\nprintf '%s\\n' 'demo.service loaded active running'\n",
    );
    let out = f.check(
        &["doctor", "--shell", "bash", "--line", "systemctl status demo", "--json"],
        0,
    );
    let json: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(json["binding"], "unknown");
    assert_eq!(json["command"], "systemctl");
    assert_eq!(json["cache"]["present"], false);
    assert_eq!(json["live"]["matches"], 1);
    assert_eq!(json["live"]["sources"][0]["status"], "success");
    assert!(!f.cache().exists(), "doctor should inspect cache without creating it");
    let human = f.check(&["doctor", "--shell", "fish", "--line", "systemctl status demo"], 0);
    assert!(human.contains("Live lookup   matches") && human.contains("help-fallback"));

    let out = f
        .command()
        .env("PATH", &f.dir)
        .args(["live", "--explain", "--json", "--line", "systemctl status "])
        .output()
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["sources"][0]["status"], "tool-missing");
    assert_eq!(json["status"], "lookup-failed");
}

#[test]
fn generated_shell_files_treat_descriptions_as_data() {
    let f = Fixture::new();
    let mut cmd = complete_me::model::Command::new("fixture-safe");
    let mut sub = complete_me::model::Command::new("start");
    sub.desc =
        "Shows literal substitutions $(printf 'fixture-output') and backticks `printf fixture-output`.".to_string();
    cmd.subcommands.insert("start".to_string(), sub);
    for (shell, source, flag) in [
        ("fish", complete_me::render::fish(&cmd).unwrap(), "--no-config"),
        ("bash", complete_me::render::bash(&cmd).unwrap(), "--norc"),
        ("zsh", complete_me::render::zsh(&cmd).unwrap(), "-f"),
    ] {
        if complete_me::util::which(shell).is_none() {
            continue;
        }
        let path = f.dir.join(format!("fixture.{shell}"));
        std::fs::write(&path, source).unwrap();
        let line = if shell == "zsh" {
            "autoload -Uz compinit; compinit -i -D; source \"$1\""
        } else if shell == "fish" {
            "source $argv[1]"
        } else {
            "source \"$1\""
        };
        let mut process = Command::new(shell);
        process.current_dir(Path::new(&f.dir)).args([flag, "-c", line]);
        if shell != "fish" {
            process.arg("fixture");
        }
        let result = process.arg(&path).output().unwrap();
        assert!(
            result.status.success(),
            "{shell}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(
            result.stdout.is_empty(),
            "{shell} produced output when sourcing description text"
        );
    }
}

#[test]
fn aliases_preserve_unmanaged_functions_and_round_trip_managed_definitions() {
    let f = Fixture::new();
    let fish = f.dir.join("config/fish/functions");
    std::fs::create_dir_all(&fish).unwrap();
    std::fs::write(fish.join("existing.fish"), "function existing; end").unwrap();
    f.check(&["alias", "existing", "echo"], 1);
    f.check(&["alias", "--rm", "existing"], 0);
    assert_eq!(
        std::fs::read_to_string(fish.join("existing.fish")).unwrap(),
        "function existing; end"
    );
    f.check(&["alias", "--shell", "all", "hello-lab", "printf", "hello"], 0);
    assert!(f.check(&["alias", "--list"], 0).contains("hello-lab"));
    for shell in ["bash", "zsh"] {
        let p = f.dir.join("config/complete-me").join(format!("aliases.{shell}"));
        assert!(std::fs::read_to_string(&p).unwrap().contains("hello-lab"));
    }
    f.check(&["alias", "--rm", "hello-lab"], 0);
    assert!(!fish.join("hello-lab.fish").exists());
    assert!(!f.check(&["alias", "--list"], 0).contains("hello-lab"));
}
