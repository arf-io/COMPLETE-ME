"""Fish syntax snippets (as ,abbreviations that expand in place) and a bash→fish phrasebook.

`%` marks where the cursor lands after expansion.
"""
from __future__ import annotations

import os
import shutil
import subprocess
import sys

SNIPPETS = [
    # trigger, description, expansion
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
    (",fna", "function with parsed options (argparse)",
     "function % --description ''\n    argparse h/help 'n/name=' -- $argv\n    or return\n    if set -q _flag_help\n        echo \"usage: \"(status function)\" [-n NAME] ARGS…\"\n        return 0\n    end\n    echo $_flag_name $argv\nend"),
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
    (",findgit", "find, skipping .git and node_modules",
     "find . \\( -path ./.git -o -path ./node_modules \\) -prune -o -type f -name '%' -print"),
    (",xargs", "safe find | xargs for odd file names", "find . -type f -name '%' -print0 | xargs -0 CMD"),
    (",rgf", "ripgrep: list files that match", "rg -l '%'"),
    (",rgr", "ripgrep: search and preview a replacement", "rg '%' -r 'REPLACEMENT'"),
    (",sedi", "replace text in a file in place", "sed -i -E 's/%//g' FILE"),
    (",tunnel", "ssh local port forward", "ssh -N -L %:localhost:PORT HOST"),
]

PHRASEBOOK = [
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
]


def fish_abbrs() -> str:
    out = []
    for trig, desc, body in SNIPPETS:
        esc = body.replace("\\", "\\\\").replace("'", "\\'")
        out.append(f"abbr -a --set-cursor -- {trig} '{esc}'  # {desc}")
    return "\n".join(out)


def pick() -> int:
    rows = [f"{t}\t{d}\t{b.replace(chr(10), '⏎')}" for t, d, b in SNIPPETS]
    rows += [f"bash: {b}\tfish: {f}\t{f}" for b, f in PHRASEBOOK]
    if not shutil.which("fzf"):
        print("\n".join(r.split("\t")[0] + "  " + r.split("\t")[1] for r in rows), file=sys.stderr)
        return 1
    p = subprocess.run(
        ["fzf", "--delimiter", "\t", "--with-nth", "1,2", "--prompt", "snippet › ", "--height", "60%",
         "--layout", "reverse", "--border", "rounded",
         "--header", "fish snippets and the bash→fish phrasebook · ENTER inserts",
         "--preview", "printf '%s\\n' {3} | sed 's/⏎/\\n/g'", "--preview-window", "down,40%,wrap"],
        input="\n".join(rows), stdout=subprocess.PIPE, text=True, env=dict(os.environ, SHELL="/bin/sh"))
    if p.returncode != 0 or not p.stdout.strip():
        return 130
    body = p.stdout.rstrip("\n").split("\t")[2].replace("⏎", "\n")
    if " (" in body and body.startswith(("cmd", "complete-me")):
        body = body.split("   (")[0]
    print(body if "%" in body else body + "%", end="")
    return 0


def register(sp):
    s = sp.add_parser("snippets", help="fish syntax snippets and bash→fish phrasebook")
    ss = s.add_subparsers(dest="snip_cmd", required=True)
    ss.add_parser("list", help="show every ,snippet").set_defaults(
        fn=lambda a: print("\n".join(f"{t:10} {d}" for t, d, _ in SNIPPETS)))
    ss.add_parser("phrasebook", help="bash → fish translations").set_defaults(
        fn=lambda a: print("\n".join(f"{b:34} →  {f}" for b, f in PHRASEBOOK)))
    ss.add_parser("fish-abbrs", help="(used by the installer) fish abbr definitions").set_defaults(
        fn=lambda a: print(fish_abbrs()))
    ss.add_parser("pick", help="fzf picker; prints the chosen snippet").set_defaults(fn=lambda a: pick())
