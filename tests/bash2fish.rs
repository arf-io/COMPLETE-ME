//! The built-in bash → fish translator, against babelfish and against real shells.
//!
//! tests/fixtures/bash2fish/NAME.sh is the corpus. tests/golden/bash2fish/NAME.fish is what
//! babelfish 1.2.1 printed for it (NAME.unsupported where it gave up). Every case must either match
//! babelfish byte for byte, or be listed in DIVERGES with the reason ours is different. And the
//! cases in RUN must print the same in bash as their translation prints in fish.
use complete_me::sh;
use std::path::Path;

/// Cases where we deliberately print something other than babelfish, and why.
const DIVERGES: &[(&str, &str)] = &[
    ("bf-chruby", "eval \"$(…)\" evaluates bash, so the output is translated before it runs; here-documents use printf"),
    ("bf-nix-index", "here-documents: printf '%s' instead of echo, which added a second newline"),
    ("all-args", "\"$@\" stays separate words ($argv), not one joined \"$argv\""),
    ("shift", "\"$@\" stays separate words"),
    ("shift-n", "shift 2 removes two arguments"),
    ("heredoc", "here-documents: printf '%s' instead of echo"),
    ("heredoc-quoted", "here-documents: printf '%s' instead of echo"),
    ("or-block", "{ …; } needs begin/end in fish, or the second command always runs"),
    ("assign-empty", "x= sets an empty value (babelfish printed nothing)"),
    ("assign-tilde", "~ still expands in an assignment"),
    ("eval-subst", "eval \"$(…)\" evaluates bash, so the output is translated before it runs"),
    ("while-read-r", "fish's read has no -r (it never mangles backslashes)"),
    ("read-prompt", "fish's read spells the prompt -P"),
    ("loop-range", "{1..3} becomes (seq 1 3)"),
    ("loop-range-step", "{1..10..3} becomes (seq 1 3 10)"),
    ("loop-range-down", "{3..1} becomes (seq 3 -1 1)"),
    ("zero", "$0 is (status filename); babelfish wrote $argv[0], which fish rejects"),
    ("replace-glob", "glob patterns in ${x/pat/…} are patterns, not literal text"),
    ("ansi-c", "$'\\t' is decoded (babelfish left it as the two characters \\ and t)"),
    ("array-set", "a[1]=Y sets that element (babelfish replaced the whole array)"),
    ("replace-anchor", "${x/#pre/…} and ${x/%suf/…} anchor (babelfish replaced the text '#pre')"),
    ("test-pattern", "[[ x == h* ]] matches a pattern (babelfish compared with the string 'h*')"),
    ("phrase-match", "[[ $a == b* ]] matches a pattern (babelfish compared with the string 'b*')"),
    ("read-t", "refused: fish's read has no timeout (babelfish passed -t through, which fish rejects)"),
    ("set-e", "bash `set -e`/`-o pipefail` would erase variables in fish: refused"),
    ("phrase-count", "$# is (count $argv)"),
    ("positional", "$# is (count $argv)"),
];

/// Cases babelfish refuses that we refuse too (everything else it refuses, we translate).
const STILL_UNSUPPORTED: &[&str] = &["loop-cstyle-continue", "read-t", "select", "coproc", "declare-assoc", "glob-ext", "set-e"];

fn corpus() -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = std::fs::read_dir("tests/fixtures/bash2fish")
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "sh"))
        .map(|p| (p.file_stem().unwrap().to_string_lossy().into_owned(), std::fs::read_to_string(&p).unwrap()))
        .collect();
    v.sort();
    v
}

/// Translated the way babelfish names itself, so `source` lines compare.
fn ours(src: &str) -> Result<String, String> {
    let f = sh::parse::parse(src)?;
    sh::fish::translate_with(&f, "babelfish")
}

#[test]
fn matches_babelfish_byte_for_byte() {
    let mut same = 0;
    let mut total = 0;
    let mut fails = vec![];
    for (name, src) in corpus() {
        let Ok(want) = std::fs::read_to_string(Path::new("tests/golden/bash2fish").join(format!("{name}.fish"))) else { continue };
        total += 1;
        let diverges = DIVERGES.iter().any(|(n, _)| *n == name);
        match ours(&src) {
            Ok(got) if got == want => {
                same += 1;
                if diverges {
                    fails.push(format!("{name} is listed in DIVERGES but matches babelfish: drop it from the list"));
                }
            }
            _ if diverges => {}
            Ok(got) => fails.push(format!("--- {name}\n--- babelfish\n{want}--- ours\n{got}")),
            Err(e) => fails.push(format!("--- {name}: {e}")),
        }
    }
    eprintln!("bash2fish: {same}/{total} byte-identical to babelfish; {} deliberate differences", total - same);
    assert!(fails.is_empty(), "{}", fails.join("\n"));
}

#[test]
fn goes_further_than_babelfish() {
    let mut now = vec![];
    for (name, src) in corpus() {
        if !Path::new("tests/golden/bash2fish").join(format!("{name}.unsupported")).exists() {
            continue;
        }
        let r = ours(&src);
        if STILL_UNSUPPORTED.contains(&name.as_str()) {
            assert!(r.is_err(), "{name} should be refused, got:\n{}", r.unwrap());
        } else {
            assert!(r.is_ok(), "{name}: {}", r.unwrap_err());
            now.push(name);
        }
    }
    eprintln!("bash2fish: translates {} cases babelfish refuses: {}", now.len(), now.join(" "));
}

// ---------------------------------------------------------------- run both

fn have(c: &str) -> bool {
    complete_me::util::which(c).is_some()
}

/// Run `code` in `shell` inside a fresh scratch directory with the same files every time.
fn run(shell: &str, code: &str, tag: &str) -> (String, i32) {
    // the same path for bash and fish: $HOME and $PWD show up in output
    let dir = std::env::temp_dir().join(format!("cm-b2f-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("input.txt"), "one\nTwo\nthree one\n").unwrap();
    std::fs::write(dir.join("env.sh"), "ENV_FROM_FILE=loaded\n").unwrap();
    for f in ["a.txt", "b.txt"] {
        std::fs::write(dir.join(f), "x\ny\n").unwrap();
    }
    let mut c = std::process::Command::new(shell);
    if shell == "fish" {
        c.args(["--no-config", "-c", code]);
    } else {
        c.args(["--norc", "--noprofile", "-c", code, "script-name"]);
    }
    let bin = Path::new(env!("CARGO_BIN_EXE_complete-me")).parent().unwrap().to_path_buf();
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap_or_default());
    c.current_dir(&dir).env("PATH", path).env("HOME", &dir).env("LC_ALL", "C").env_remove("UNSET_VAR");
    let out = c.stdin(std::process::Stdio::null()).output().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    (String::from_utf8_lossy(&out.stdout).into_owned(), out.status.code().unwrap_or(-1))
}

/// Cases that print something deterministic, so bash and fish can be compared.
const RUN: &[&str] = &["loop-words", "loop-files", "loop-range", "loop-range-step", "loop-range-down", "loop-cstyle", "loop-seq",
    "while-read", "while-read-r", "while-count", "until", "if-file", "if-elif", "test-double", "test-pattern", "test-equal-single",
    "test-regex", "test-not", "and-or", "or-block", "negate", "subst", "subst-backtick", "subst-nested", "subst-file", "arith",
    "arith-div", "arith-pow", "arith-cmp", "arith-incr", "arith-addeq", "let", "default", "default-dash", "alternate", "length",
    "strip-suffix", "strip-prefix", "strip-prefix-short", "strip-suffix-long", "replace", "replace-all", "replace-glob",
    "replace-anchor", "upper-lower", "slice", "indirect", "array", "array-all", "array-append", "array-set", "positional", "all-args",
    "shift", "shift-n", "function-kw", "function-local", "export", "assign-prefix", "assign-empty", "assign-tilde", "redirect",
    "redirect-append", "redirect-err", "redirect-both", "redirect-all", "pipe-all", "heredoc", "heredoc-quoted", "heredoc-dash",
    "herestring", "procsubst", "case", "case-quoted", "background", "last-pid", "status", "pid", "ansi-c", "eval-subst", "unset",
    "cd-and", "subshell", "zero", "comment-trailing", "comment-lines", "brace-list", "escape", "multiline", "semis-done", "printf",
    "grep-pipe", "nested-quotes", "declare", "readonly"];

#[test]
fn bash_and_fish_agree() {
    if !have("bash") || !have("fish") {
        eprintln!("bash or fish missing; skipping");
        return;
    }
    let corpus: std::collections::HashMap<String, String> = corpus().into_iter().collect();
    let mut fails = vec![];
    for name in RUN {
        let src = corpus.get(*name).unwrap_or_else(|| panic!("no corpus case {name}"));
        let fish_code = match sh::to_fish(src) {
            Ok(c) => c,
            Err(e) => {
                fails.push(format!("{name}: not translated: {e}"));
                continue;
            }
        };
        let (bo, bs) = run("bash", src, name);
        let (fo, fs) = run("fish", &fish_code, name);
        if bo != fo || (bs == 0) != (fs == 0) {
            fails.push(format!("--- {name}\n{src}--- fish\n{fish_code}\n--- bash said ({bs})\n{bo}--- fish said ({fs})\n{fo}"));
        }
    }
    eprintln!("bash2fish: {}/{} run identically in bash and fish", RUN.len() - fails.len(), RUN.len());
    assert!(fails.is_empty(), "{} of {} differ:\n{}", fails.len(), RUN.len(), fails.join("\n"));
}
