"""complete-me alias / func: shell aliases and functions that pass their arguments through
and keep the wrapped command's TAB completions.

  complete-me alias gco git checkout          # gco <TAB> completes branches like git checkout
  complete-me alias logs 'journalctl -u $1 -f' # $1 goes where you put it
  complete-me alias --abbr gs git status      # fish abbreviation: expands as you type
  complete-me func mkcd 'mkdir -p $1; and cd $1'

Fish gets an autoloaded file in ~/.config/fish/functions using --wraps.
Bash/zsh get ~/.config/complete-me/aliases.{bash,zsh}, which the installer sources.
"""
from __future__ import annotations

import os
import re
import shlex
import subprocess
import sys
from pathlib import Path

NAME = re.compile(r"^[A-Za-z_,.][\w.,+-]*$")
FISH_RESERVED = {"and", "or", "not", "if", "else", "end", "for", "while", "function", "switch", "case", "begin",
                 "return", "break", "continue", "set", "test", "string", "builtin", "command", "exec", "time",
                 "status", "math", "count", "cd", "eval", "source", "read"}
MARK = "# Created by complete-me"
PLACEHOLDER = re.compile(r"\$(?:argv(?:\[\d+\])?|[1-9@*])|\"\$@\"")


def fish_functions_dir() -> Path:
    base = Path(os.environ.get("XDG_CONFIG_HOME") or Path.home() / ".config")
    return base / "fish" / "functions"


def sh_file(shell: str) -> Path:
    base = Path(os.environ.get("XDG_CONFIG_HOME") or Path.home() / ".config")
    return base / "complete-me" / f"aliases.{shell}"


def to_fish(body: str) -> str:
    body = body.replace('"$@"', "$argv").replace("$@", "$argv").replace("$*", "$argv")
    return re.sub(r"\$([1-9])", r"$argv[\1]", body)


def to_sh(body: str) -> str:
    body = re.sub(r"\$argv\[(\d)\]", r'"$\1"', body).replace("$argv", '"$@"')
    body = re.sub(r'(?<!")\$([1-9])(?!")', r'"$\1"', body)
    return body.replace("; and ", " && ").replace("; or ", " || ")


def wraps_of(body: str) -> str:
    """The command words the alias stands for, up to the first placeholder or control operator."""
    words = []
    for w in shlex.split(body.split(";")[0].split("&&")[0].split("|")[0], posix=True) if body.strip() else []:
        if PLACEHOLDER.search(w) or w in ("command", "builtin"):
            if w in ("command", "builtin"):
                continue
            break
        words.append(w)
    return " ".join(shlex.quote(w) for w in words)


def check_name(name: str) -> None:
    if not NAME.match(name) or name in FISH_RESERVED:
        raise SystemExit(f"complete-me: {name!r} can't be used as a name (reserved or unsafe)")


def write_fish(name: str, body: str, desc: str, is_alias: bool) -> Path:
    fbody = to_fish(body)
    first = fbody.split()[0] if fbody.split() else ""
    if first == name:
        fbody = "command " + fbody          # alias ls 'ls -la' must not call itself
    if is_alias and not PLACEHOLDER.search(body):
        fbody += " $argv"
    elif is_alias and not re.search(r"\$argv(?!\[)|\$[@*]", body):
        highest = max(int(n) for n in re.findall(r"\$(?:argv\[)?([1-9])", body))
        fbody += f" $argv[{highest + 1}..]"      # pass the rest through too
    wraps = wraps_of(body)
    head = f"function {name}"
    if wraps and shlex.split(wraps)[0] != name:
        head += f" --wraps {fq(wraps)}"
    head += f" --description {fq(desc)}"
    indented = "\n".join("    " + l for l in fbody.split("\n"))
    path = fish_functions_dir() / f"{name}.fish"
    if path.exists() and MARK not in path.read_text()[:200]:
        raise SystemExit(f"complete-me: {path} exists and was not made by complete-me; not overwriting")
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(f"{MARK} ({'alias' if is_alias else 'func'}). Remove: complete-me alias --rm {name}\n"
                    f"{head}\n{indented}\nend\n")
    return path


def fq(s: str) -> str:
    return "'" + s.replace("\\", "\\\\").replace("'", "\\'") + "'"


def write_sh(shell: str, name: str, body: str, is_alias: bool) -> Path:
    sbody = to_sh(body)
    if is_alias and not PLACEHOLDER.search(body):
        sbody += ' "$@"'
    elif is_alias and not re.search(r"\$argv(?!\[)|\$[@*]", body):
        highest = max(int(n) for n in re.findall(r"\$(?:argv\[)?([1-9])", body))
        sbody += f' "${{@:{highest + 1}}}"'
    if sbody.split()[0] == name:
        sbody = "command " + sbody
    wraps = wraps_of(body)
    path = sh_file(shell)
    path.parent.mkdir(parents=True, exist_ok=True)
    lines = path.read_text().split("\n") if path.exists() else [f"{MARK}: aliases and functions. Sourced from your rc file."]
    lines = [l for l in lines if not l.endswith(f"# cm:{name}")]
    lines.append(f"{name}() {{ {sbody}; }}  # cm:{name}")
    if wraps:
        if shell == "bash":
            lines.append(f"_complete_me_alias_register {name} {wraps}  # cm:{name}")
        else:
            lines.append(f"compdef {name}={shlex.split(wraps)[0]} 2>/dev/null  # cm:{name}")
    path.write_text("\n".join(l for l in lines if l) + "\n")
    return path


def remove(name: str) -> None:
    p = fish_functions_dir() / f"{name}.fish"
    if p.exists() and MARK in p.read_text()[:200]:
        p.unlink()
        print(f"removed {p}")
    for shell in ("bash", "zsh"):
        f = sh_file(shell)
        if f.exists():
            lines = f.read_text().split("\n")
            kept = [l for l in lines if not l.endswith(f"# cm:{name}")]
            if kept != lines:
                f.write_text("\n".join(kept))
                print(f"removed {name} from {f}")
    abbr = abbr_file()
    if abbr.exists():
        lines = abbr.read_text().split("\n")
        kept = [l for l in lines if not l.endswith(f"# cm:{name}")]
        if kept != lines:
            abbr.write_text("\n".join(kept))
            print(f"removed abbreviation {name}")


def abbr_file() -> Path:
    base = Path(os.environ.get("XDG_CONFIG_HOME") or Path.home() / ".config")
    return base / "fish" / "conf.d" / "complete_me_abbrs.fish"


def write_abbr(name: str, body: str) -> Path:
    p = abbr_file()
    lines = p.read_text().split("\n") if p.exists() else [f"{MARK}: abbreviations (complete-me alias --abbr)."]
    lines = [l for l in lines if l and not l.endswith(f"# cm:{name}")]
    cursor = " --set-cursor" if "%" in body else ""
    lines.append(f"abbr -a{cursor} -- {name} {fq(body)}  # cm:{name}")
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text("\n".join(lines) + "\n")
    return p


def list_all() -> None:
    d = fish_functions_dir()
    for p in sorted(d.glob("*.fish")) if d.exists() else []:
        t = p.read_text()
        if MARK in t[:200]:
            body = "; ".join(l.strip() for l in t.split("\n")[2:-2])
            print(f"{p.stem:14} fish  {body}")
    if abbr_file().exists():
        for l in abbr_file().read_text().split("\n")[1:]:
            m = re.match(r"abbr -a(?: --set-cursor)? -- (\S+) (.*)  # cm:", l)
            if m:
                print(f"{m.group(1):14} abbr  {m.group(2)}")


def run(a, is_alias: bool) -> int:
    if a.list:
        list_all()
        return 0
    if a.rm:
        for n in a.rm:
            remove(n)
        return 0
    if not a.name:
        print("usage: complete-me alias NAME COMMAND…   (see --help)", file=sys.stderr)
        return 2
    check_name(a.name)
    body = " ".join(a.body) if len(a.body) != 1 else a.body[0]
    if not body and not is_alias:
        body = sys.stdin.read() if not sys.stdin.isatty() else ""
        if not body:
            return edit_new(a.name, a.wraps)
    if not body:
        print("complete-me: give the command the alias stands for", file=sys.stderr)
        return 2
    if a.wraps and not is_alias:
        body = body  # explicit wraps handled below
    desc = a.desc or (f"alias {a.name}={body}" if is_alias else f"function {a.name}")
    shells = ["fish", "bash", "zsh"] if a.shell == "all" else [a.shell]
    for sh in shells:
        if sh == "fish":
            if a.abbr:
                print(f"fish abbreviation -> {write_abbr(a.name, body)}  (new shells; or run: source it)")
            else:
                p = write_fish(a.name, body, desc, is_alias)
                if a.wraps:
                    t = p.read_text()
                    t = re.sub(r" --wraps '[^']*'", "", t, count=1).replace(f"function {a.name}", f"function {a.name} --wraps {fq(a.wraps)}", 1)
                    p.write_text(t)
                print(f"fish -> {p}  (available immediately in every fish; TAB completes like: {a.wraps or wraps_of(body) or 'n/a'})")
        else:
            print(f"{sh} -> {write_sh(sh, a.name, body, is_alias)}")
    return 0


def edit_new(name: str, wraps: str | None) -> int:
    p = fish_functions_dir() / f"{name}.fish"
    if not p.exists():
        p.parent.mkdir(parents=True, exist_ok=True)
        w = f" --wraps {fq(wraps)}" if wraps else ""
        p.write_text(f"{MARK} (func). Remove: complete-me alias --rm {name}\n"
                     f"function {name}{w} --description ''\n"
                     f"    argparse h/help -- $argv\n    or return\n"
                     f"    if set -q _flag_help\n        echo \"usage: {name} ARGS…\"\n        return 0\n    end\n"
                     f"    # $argv holds the remaining arguments; $argv[1] is the first\n    \nend\n")
    editor = os.environ.get("VISUAL") or os.environ.get("EDITOR") or "nano"
    return subprocess.call(shlex.split(editor) + [str(p)])


def register(sp):
    for kind in ("alias", "func"):
        p = sp.add_parser(kind, help=("make an alias that passes args and keeps completions" if kind == "alias"
                                      else "make a shell function (opens $EDITOR with no body)"),
                          description=__doc__, formatter_class=__import__("argparse").RawDescriptionHelpFormatter)
        p.add_argument("name", nargs="?")
        p.add_argument("body", nargs="*", help="the command; use $1 $2 … or $argv / $@ to place arguments")
        p.add_argument("--shell", choices=["fish", "bash", "zsh", "all"], default="fish")
        p.add_argument("--abbr", action="store_true", help="fish abbreviation instead of a function")
        p.add_argument("--wraps", help="complete like this command (default: inferred)")
        p.add_argument("--desc", help="description shown in completions")
        p.add_argument("--list", action="store_true", help="list what complete-me created")
        p.add_argument("--rm", nargs="+", metavar="NAME", help="remove")
        p.set_defaults(fn=(lambda a, k=kind: run(a, k == "alias")))
