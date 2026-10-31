//! Shell syntax snippets (as ,abbreviations that expand in place) and a bash→fish phrasebook.
//! Fish gets fish syntax; bash and zsh get theirs, with the same names. `%` marks where the
//! cursor lands after expansion.

pub const SNIPPETS: &[(&str, &str, &str)] = &[
    (",for", "loop over values", "for x in %\n    echo $x\nend"),
    (",forf", "loop over files matching a glob", "for f in *.%\n    echo $f\nend"),
    (",fori", "count from 1 to N", "for i in (seq %)\n    echo $i\nend"),
    (",forl", "loop over the lines of a command's output", "for line in (%)\n    echo $line\nend"),
    (",while", "read a file line by line", "while read -l line\n    echo $line\nend < %"),
    (",if", "if a test is true", "if test %\n    \nend"),
    (",ife", "if / else", "if test %\n    \nelse\n    \nend"),
    (",ifc", "if a command succeeds", "if %\n    \nend"),
    (",iff", "if a file exists", "if test -f %\n    \nend"),
    (",ifd", "if a directory exists", "if test -d %\n    \nend"),
    (",ifv", "if a variable is set", "if set -q %\n    \nend"),
    (",sw", "switch on a value", "switch %\n    case 'a*'\n        \n    case '*'\n        \nend"),
    (",fn", "define a function (passes all args)", "function % --description ''\n    \nend"),
    (",fna", "function with parsed options (argparse)", "function % --description ''\n    argparse h/help 'n/name=' -- $argv\n    or return\n    if set -q _flag_help\n        echo \"usage: \"(status function)\" [-n NAME] ARGS…\"\n        return 0\n    end\n    echo $_flag_name $argv\nend"),
    (",wrap", "function that wraps a command and keeps its completions", "function % --wraps CMD\n    CMD $argv\nend"),
    (",ev", "run a function when an event fires", "function % --on-event fish_postexec\n    \nend"),
    (",onvar", "run a function when a variable changes", "function % --on-variable PWD\n    \nend"),
    (",set", "export a variable", "set -gx % "),
    (",path", "prepend to PATH (deduplicated)", "fish_add_path %"),
    (",sub", "capture a command's output", "set out (%)"),
    (",st", "check the last exit status", "if test $status -ne 0\n    %\nend"),
    (",and", "run the next command only if this succeeds", "%; and "),
    (",or", "run the next command only if this fails", "%; or "),
    (",psub", "pass command output as a file (bash <(…))", "(% | psub)"),
    (",repl", "string replace (regex)", "string replace -r -- '%' '' $x"),
    (",split", "split a string", "string split -- '%' $x"),
    (",match", "regex match with capture groups", "string match -rg -- '%' $x"),
    (",math", "arithmetic", "math \"%\""),
    (",count", "count items", "count %"),
    (",ask", "ask the user a yes/no question", "read -l -P '% [y/N] ' answer\nif string match -qi 'y*' -- $answer\n    \nend"),
    (",trap", "run cleanup when the shell exits", "function __cleanup --on-event fish_exit\n    %\nend"),
    (",bg", "run in the background and forget it", "% &; disown"),
    (",findx", "find files and run a command on each", "find . -type f -name '%' -exec CMD {} \\;"),
    (",findb", "find files and run one command on all of them", "find . -type f -name '%' -exec CMD {} +"),
    (",findn", "find files changed in the last day", "find . -type f -mtime -1 %"),
    (",findbig", "find big files", "find % -type f -size +100M -exec ls -lh {} +"),
    (",findgit", "find, skipping .git and node_modules", "find . \\( -path ./.git -o -path ./node_modules \\) -prune -o -type f -name '%' -print"),
    (",xargs", "safe find | xargs for odd file names", "find . -type f -name '%' -print0 | xargs -0 CMD"),
    (",rgf", "ripgrep: list files that match", "rg -l '%'"),
    (",rgr", "ripgrep: search and preview a replacement", "rg '%' -r 'REPLACEMENT'"),
    (",sedi", "replace text in a file in place", "sed -i -E 's/%//g' FILE"),
    (",tunnel", "ssh local port forward", "ssh -N -L %:localhost:PORT HOST"),
];
/// bash (and zsh: everything here is valid in both unless it is in ZSH_ONLY).
pub const SH_SNIPPETS: &[(&str, &str, &str)] = &[
    (",for", "loop over values", "for x in %; do\n    echo \"$x\"\ndone"),
    (",forf", "loop over files matching a glob", "for f in *.%; do\n    echo \"$f\"\ndone"),
    (",fori", "count from 1 to N", "for ((i = 1; i <= %; i++)); do\n    echo \"$i\"\ndone"),
    (",forl", "loop over the lines of a command's output", "while IFS= read -r line; do\n    echo \"$line\"\ndone < <(%)"),
    (",while", "read a file line by line", "while IFS= read -r line; do\n    echo \"$line\"\ndone < %"),
    (",if", "if a test is true", "if [[ % ]]; then\n    \nfi"),
    (",ife", "if / else", "if [[ % ]]; then\n    \nelse\n    \nfi"),
    (",ifc", "if a command succeeds", "if %; then\n    \nfi"),
    (",iff", "if a file exists", "if [[ -f % ]]; then\n    \nfi"),
    (",ifd", "if a directory exists", "if [[ -d % ]]; then\n    \nfi"),
    (",ifv", "if a variable is set", "if [[ -n ${%+x} ]]; then\n    \nfi"),
    (",sw", "case on a value", "case % in\n    a*) ;;\n    *) ;;\nesac"),
    (",fn", "define a function (passes all args)", "%() {\n    \"$@\"\n}"),
    (",fna", "function with parsed options (getopts)", "%() {\n    local OPTIND opt name=\n    while getopts hn: opt; do\n        case $opt in\n            h) echo \"usage: ${FUNCNAME[0]:-$0} [-n NAME] ARGS…\"; return 0;;\n            n) name=$OPTARG;;\n            *) return 2;;\n        esac\n    done\n    shift $((OPTIND - 1))\n    echo \"$name\" \"$@\"\n}"),
    (",wrap", "function that wraps a command (complete-me func keeps its completions)", "%() {\n    command CMD \"$@\"\n}"),
    (",set", "export a variable", "export %="),
    (",path", "prepend to PATH (deduplicated)", "d=%; case :$PATH: in *:\"$d\":*) ;; *) PATH=$d:$PATH;; esac"),
    (",sub", "capture a command's output", "out=$(%)"),
    (",st", "check the last exit status", "if (( $? != 0 )); then\n    %\nfi"),
    (",and", "run the next command only if this succeeds", "% && "),
    (",or", "run the next command only if this fails", "% || "),
    (",psub", "pass command output as a file", "<(%)"),
    (",repl", "replace text in a variable", "${x//%/}"),
    (",split", "split a string into an array", "IFS='%' read -ra parts <<< \"$x\""),
    (",match", "regex match with capture groups", "if [[ $x =~ % ]]; then\n    echo \"${BASH_REMATCH[1]}\"\nfi"),
    (",math", "arithmetic", "$(( % ))"),
    (",count", "count the items of an array", "${#%[@]}"),
    (",ask", "ask the user a yes/no question", "read -r -p '% [y/N] ' answer\nif [[ $answer == [yY]* ]]; then\n    \nfi"),
    (",trap", "run cleanup when the shell exits", "trap '%' EXIT"),
    (",strict", "strict mode for scripts", "set -euo pipefail%"),
    (",bg", "run in the background and forget it", "% & disown"),
    (",heredoc", "multi-line text into a command", "cat <<'EOF'\n%\nEOF"),
    (",default", "a variable with a default", "${%:-default}"),
    (",findx", "find files and run a command on each", "find . -type f -name '%' -exec CMD {} \\;"),
    (",findb", "find files and run one command on all of them", "find . -type f -name '%' -exec CMD {} +"),
    (",findn", "find files changed in the last day", "find . -type f -mtime -1 %"),
    (",findbig", "find big files", "find % -type f -size +100M -exec ls -lh {} +"),
    (",findgit", "find, skipping .git and node_modules", "find . \\( -path ./.git -o -path ./node_modules \\) -prune -o -type f -name '%' -print"),
    (",xargs", "safe find | xargs for odd file names", "find . -type f -name '%' -print0 | xargs -0 CMD"),
    (",rgf", "ripgrep: list files that match", "rg -l '%'"),
    (",rgr", "ripgrep: search and preview a replacement", "rg '%' -r 'REPLACEMENT'"),
    (",sedi", "replace text in a file in place", "sed -i -E 's/%//g' FILE"),
    (",tunnel", "ssh local port forward", "ssh -N -L %:localhost:PORT HOST"),
];

/// zsh spellings where zsh has a nicer way than the bash one (same names, so they replace it).
pub const ZSH_ONLY: &[(&str, &str, &str)] = &[
    (",fori", "count from 1 to N", "for i in {1..%}; do\n    echo \"$i\"\ndone"),
    (",forl", "loop over the lines of a command's output", "for line in ${(f)\"$(%)\"}; do\n    echo \"$line\"\ndone"),
    (",fn", "define a function (passes all args)", "function % {\n    \"$@\"\n}"),
    (",split", "split a string into an array", "parts=(${(s:%:)x})"),
    (",match", "regex match with capture groups", "if [[ $x =~ % ]]; then\n    echo \"$match[1]\"\nfi"),
    (",ask", "ask the user a yes/no question", "read -q '?% [y/N] ' && {\n    \n}"),
    (",count", "count the items of an array", "${#%}"),
    (",glob", "recursive glob, files only (zsh qualifiers)", "**/*.%(.N)"),
];

/// The snippet set for a shell, in display order.
pub fn for_shell(shell: &str) -> Vec<(&'static str, &'static str, &'static str)> {
    match shell {
        "bash" => SH_SNIPPETS.to_vec(),
        "zsh" => {
            let mut v: Vec<_> = SH_SNIPPETS.iter().map(|s| *ZSH_ONLY.iter().find(|z| z.0 == s.0).unwrap_or(s)).collect();
            v.extend(ZSH_ONLY.iter().filter(|z| !SH_SNIPPETS.iter().any(|s| s.0 == z.0)).copied());
            v
        }
        _ => SNIPPETS.to_vec(),
    }
}

pub const PHRASEBOOK: &[(&str, &str)] = &[
    ("export X=1", "set -gx X 1"),
    ("X=$(cmd)", "set X (cmd)"),
    ("$?", "$status"),
    ("$@  /  $*", "$argv"),
    ("$1", "$argv[1]"),
    ("$#", "(count $argv)"),
    ("for i in {1..10}; do …; done", "for i in (seq 10); …; end"),
    ("if [ -f x ]; then …; fi", "if test -f x; …; end"),
    ("[[ $a == b* ]]", "string match -q 'b*' -- $a"),
    ("cmd1 && cmd2", "cmd1 && cmd2   (or: cmd1; and cmd2)"),
    ("cmd 2>&1 | less", "cmd &| less"),
    ("cmd > /dev/null 2>&1", "cmd &>/dev/null"),
    ("diff <(a) <(b)", "diff (a | psub) (b | psub)"),
    ("${var:-default}", "set -q var; or set var default"),
    ("${#var}", "string length -- $var"),
    ("${var/foo/bar}", "string replace foo bar -- $var"),
    ("${var%.txt}", "path change-extension '' $var"),
    ("$(basename $f)", "path basename $f"),
    ("alias ll='ls -l'", "complete-me alias ll 'ls -l'   (or: abbr -a ll 'ls -l')"),
    ("source ~/.bashrc", "source ~/.config/fish/config.fish"),
    ("function f() { … }", "function f; …; end"),
    ("local x=1", "set -l x 1"),
    ("unset X", "set -e X"),
    ("echo $PATH | tr : '\\n'", "printf '%s\\n' $PATH"),
    ("while read line; do …; done < f", "while read -l line; …; end < f"),
    ("case $x in a) …;; esac", "switch $x; case a; …; end"),
    ("trap 'cleanup' EXIT", "function cleanup --on-event fish_exit; …; end"),
    ("cd -", "cd -   (also: prevd / nextd, alt-left / alt-right)"),
    ("!!", "type it: complete-me installs !! and !$ abbreviations"),
];

fn pad(s: &str, w: usize) -> String {
    let n = s.chars().count();
    if n >= w { s.to_string() } else { format!("{s}{}", " ".repeat(w - n)) }
}

pub fn fish_abbrs() -> String {
    SNIPPETS
        .iter()
        .map(|(t, d, b)| format!("abbr -a --set-cursor -- {t} '{}'  # {d}", b.replace('\\', "\\\\").replace('\'', "\\'")))
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn list(shell: &str) -> String {
    for_shell(shell).iter().map(|(t, d, _)| format!("{} {d}", pad(t, 10))).collect::<Vec<_>>().join("\n")
}

pub fn phrasebook() -> String {
    PHRASEBOOK.iter().map(|(b, f)| format!("{} →  {f}", pad(b, 34))).collect::<Vec<_>>().join("\n")
}

pub fn pick(shell: &str) -> i32 {
    use std::io::Write;
    let mut rows: Vec<String> = for_shell(shell).iter().map(|(t, d, b)| format!("{t}\t{d}\t{}", b.replace('\n', "⏎"))).collect();
    let fish = !matches!(shell, "bash" | "zsh");
    if fish {
        rows.extend(PHRASEBOOK.iter().map(|(b, f)| format!("bash: {b}\tfish: {f}\t{f}")));
    }
    let header = if fish { "fish snippets and the bash→fish phrasebook · ENTER inserts".to_string() } else { format!("{shell} snippets · ENTER inserts · type ,name then space to expand in place") };
    if crate::util::which("fzf").is_none() {
        for r in &rows {
            let mut it = r.split('\t');
            eprintln!("{}  {}", it.next().unwrap_or(""), it.next().unwrap_or(""));
        }
        return 1;
    }
    let child = std::process::Command::new("fzf")
        .args(["--delimiter", "\t", "--with-nth", "1,2", "--prompt", "snippet › ", "--height", "60%", "--layout", "reverse",
            "--border", "rounded", "--header", &header,
            "--preview", "printf '%s\\n' {3} | sed 's/⏎/\\n/g'", "--preview-window", "down,40%,wrap"])
        .env("SHELL", "/bin/sh")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn();
    let Ok(mut child) = child else { return 130 };
    if let Some(mut si) = child.stdin.take() {
        let _ = si.write_all(rows.join("\n").as_bytes());
    }
    let Ok(out) = child.wait_with_output() else { return 130 };
    let text = String::from_utf8_lossy(&out.stdout).trim_end_matches('\n').to_string();
    if !out.status.success() || text.trim().is_empty() {
        return 130;
    }
    let mut body = text.split('\t').nth(2).unwrap_or("").replace('⏎', "\n");
    if fish && body.contains(" (") && (body.starts_with("cmd") || body.starts_with("complete-me")) {
        body = body.split("   (").next().unwrap_or("").to_string();
    }
    if !body.contains('%') {
        body.push('%');
    }
    print!("{body}");
    0
}

/// The body of one snippet by name (`,for`), for in-place expansion in bash/zsh.
pub fn expand(shell: &str, name: &str) -> Option<&'static str> {
    for_shell(shell).into_iter().find(|s| s.0 == name).map(|s| s.2)
}

pub fn run(args: &[String]) -> i32 {
    let a = crate::util::Args::parse(args.get(1..).unwrap_or(&[]), &["--shell"]);
    let shell = a.value("--shell").unwrap_or_else(|| "fish".into());
    match args.first().map(|s| s.as_str()) {
        Some("list") => println!("{}", list(&shell)),
        Some("phrasebook") => println!("{}", phrasebook()),
        Some("fish-abbrs") => println!("{}", fish_abbrs()),
        Some("pick") => return pick(&shell),
        Some("names") => println!("{}", for_shell(&shell).iter().map(|s| s.0).collect::<Vec<_>>().join(" ")),
        Some("expand") => match a.rest.first().and_then(|n| expand(&shell, n)) {
            Some(b) => print!("{b}"),
            None => return 1,
        },
        _ => {
            eprintln!("usage: complete-me snippets <list|phrasebook|fish-abbrs|pick|names|expand NAME> [--shell fish|bash|zsh]");
            return 2;
        }
    }
    0
}
