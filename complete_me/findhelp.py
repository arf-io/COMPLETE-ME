"""find(1): value suggestions for its tests, and a plain-English reading of a find command."""
from __future__ import annotations

import os
import re
import sys
from collections import Counter

ZERO_ARG = {
    "-empty": "is empty (a 0-byte file or an empty directory)",
    "-executable": "is executable by you", "-readable": "is readable by you", "-writable": "is writable by you",
    "-nouser": "has no known owner", "-nogroup": "has no known group", "-true": "always true", "-false": "never true",
    "-print": "print its path", "-print0": "print its path, NUL-terminated (pair with xargs -0)",
    "-ls": "list it like ls -dils", "-delete": "DELETE it", "-prune": "do not descend into it",
    "-quit": "stop searching right away", "-depth": "(process directory contents before the directory)",
    "-xdev": "(stay on this filesystem)", "-mount": "(stay on this filesystem)",
    "-daystart": "(measure times from the start of today)", "-follow": "(follow symlinks)",
    "-noleaf": "(do not optimise for Unix directory link counts)",
}
ONE_ARG = {"-name", "-iname", "-path", "-ipath", "-wholename", "-iwholename", "-regex", "-iregex", "-type",
           "-xtype", "-size", "-mtime", "-atime", "-ctime", "-mmin", "-amin", "-cmin", "-perm", "-user", "-group",
           "-uid", "-gid", "-newer", "-anewer", "-cnewer", "-links", "-inum", "-samefile", "-lname", "-ilname",
           "-fstype", "-maxdepth", "-mindepth", "-regextype", "-used", "-context", "-printf", "-fprint",
           "-fprint0", "-fls", "-newermt", "-newerct", "-newerat", "-newermm", "-files0-from"}
ACTIONS = {"-print", "-print0", "-ls", "-delete", "-prune", "-quit", "-exec", "-execdir", "-ok", "-okdir",
           "-printf", "-fprint", "-fprint0", "-fls", "-fprintf"}
EXEC = {"-exec", "-execdir", "-ok", "-okdir"}
TYPES = {"f": "regular file", "d": "directory", "l": "symbolic link", "p": "named pipe (FIFO)", "s": "socket",
         "b": "block device", "c": "character device"}
UNITS = {"c": "bytes", "k": "KiB", "M": "MiB", "G": "GiB", "b": "512-byte blocks", "w": "2-byte words", "": "512-byte blocks"}


# ------------------------------------------------------------ value suggestions

def extensions_here(limit: int = 12) -> list[tuple[str, str]]:
    counts: Counter[str] = Counter()
    try:
        for i, entry in enumerate(os.scandir(".")):
            if i > 3000:
                break
            ext = os.path.splitext(entry.name)[1]
            if ext and len(ext) <= 10 and not entry.name.startswith("."):
                counts[ext] += 1
    except OSError:
        pass
    return [(f"*{e}", f"{n} {'file' if n == 1 else 'files'} like this here") for e, n in counts.most_common(limit)]


def values_for(prev: str) -> list[tuple[str, str]]:
    if prev in ("-type", "-xtype"):
        return list(TYPES.items()) + [("f,l", "regular file or symlink (GNU)")]
    if prev == "-size":
        return [("+100M", "bigger than 100 MiB"), ("+1G", "bigger than 1 GiB"), ("-1k", "smaller than 1 KiB"),
                ("+10M", "bigger than 10 MiB"), ("0", "exactly zero size"), ("-100c", "under 100 bytes")]
    if prev in ("-mtime", "-atime", "-ctime"):
        verb = {"-mtime": "modified", "-atime": "accessed", "-ctime": "status-changed"}[prev]
        return [("-1", f"{verb} in the last 24 hours"), ("-7", f"{verb} in the last week"),
                ("+30", f"{verb} more than 30 days ago"), ("+365", f"{verb} more than a year ago"),
                ("0", f"{verb} between now and 24 hours ago")]
    if prev in ("-mmin", "-amin", "-cmin"):
        return [("-5", "in the last 5 minutes"), ("-60", "in the last hour"), ("+1440", "more than a day ago")]
    if prev == "-perm":
        return [("-u+x", "executable by its owner"), ("/a+x", "executable by anyone"), ("-o+w", "world-writable ⚠"),
                ("644", "exactly rw-r--r--"), ("755", "exactly rwxr-xr-x"), ("600", "exactly rw-------"),
                ("-4000", "setuid bit set"), ("-2000", "setgid bit set"), ("/6000", "setuid or setgid")]
    if prev in ("-name", "-iname", "-lname", "-ilname"):
        return extensions_here() + [("*.log", "log files"), (".*", "hidden files"), ("*~", "editor backups"),
                                    ("*.bak", "backup files"), ("*.tmp", "temporary files")]
    if prev in ("-path", "-ipath", "-wholename"):
        return [("*/node_modules/*", "anything inside node_modules"), ("*/.git/*", "anything inside .git"),
                ("./build/*", "inside ./build"), ("*/target/*", "inside a cargo target dir")]
    if prev in ("-maxdepth", "-mindepth"):
        return [("1", "this directory only"), ("2", "one level of subdirectories"), ("3", "two levels")]
    if prev == "-regextype":
        return [("posix-extended", "like grep -E (recommended)"), ("posix-basic", "like grep"),
                ("emacs", "the default"), ("posix-egrep", "like egrep")]
    if prev == "-printf":
        return [(r"%p\n", "path"), (r"%s\t%p\n", "size in bytes, path"), (r"%TY-%Tm-%Td %TH:%TM %p\n", "modified date, path"),
                (r"%M %u %g %p\n", "permissions, owner, group, path"), (r"%f\n", "file name only"), (r"%h\n", "parent directory")]
    if prev in ("-newermt", "-newerct", "-newerat"):
        return [("today", "since midnight"), ("yesterday", "since yesterday midnight"), ("1 hour ago", "in the last hour"),
                ("2026-01-01", "since this date")]
    return []


def operators() -> list[tuple[str, str]]:
    return [("(", "start a group: \\( … \\) (needed around -o before an action)"), (")", "end the group"),
            ("-o", "OR: either side"), ("-a", "AND (the default between tests)"), ("!", "NOT: invert the next test"),
            ("-not", "NOT: invert the next test"), ("-prune", "do not descend (e.g. -path ./.git -prune -o …)")]


# ------------------------------------------------------------ explanation

class Node:
    def __init__(self, kind, items=None, text="", action=False):
        self.kind, self.items, self.text, self.action = kind, items or [], text, action


def size_text(arg: str) -> str:
    m = re.fullmatch(r"([+-]?)(\d+)([ckMGbw]?)", arg)
    if not m:
        return f"has size {arg}"
    sign, n, unit = m.groups()
    rel = {"+": "bigger than", "-": "smaller than", "": "exactly"}[sign]
    return f"is {rel} {n} {UNITS[unit]}"


def days_text(arg: str, verb: str, unit: str) -> str:
    m = re.fullmatch(r"([+-]?)(\d+)", arg)
    if not m:
        return f"was {verb} {arg} {unit}"
    sign, n = m.groups()
    if sign == "-":
        return f"was {verb} less than {n} {unit} ago"
    if sign == "+":
        return f"was {verb} more than {n} {unit} ago"
    return f"was {verb} {n} {unit} ago (rounded down)"


def primary_text(tok: str, args: list[str]) -> str:
    a = args[0] if args else "?"
    simple = {
        "-name": f"its name matches {a!r}", "-iname": f"its name matches {a!r} (any case)",
        "-path": f"its path matches {a!r}", "-ipath": f"its path matches {a!r} (any case)",
        "-wholename": f"its path matches {a!r}", "-regex": f"its WHOLE path matches the regex {a!r}",
        "-iregex": f"its WHOLE path matches the regex {a!r} (any case)",
        "-user": f"is owned by user {a}", "-group": f"belongs to group {a}", "-newer": f"is newer than {a}",
        "-maxdepth": f"(look at most {a} level(s) deep)", "-mindepth": f"(skip the first {a} level(s))",
        "-regextype": f"(regexes use {a} syntax)", "-perm": f"has permissions {a}",
        "-links": f"has {a} hard links", "-samefile": f"is the same file as {a}", "-fstype": f"is on a {a} filesystem",
        "-printf": f"print using the format {a!r}", "-fprint": f"write its path to the file {a}",
        "-newermt": f"was modified after {a!r}",
    }
    if tok in ("-type", "-xtype"):
        kinds = [TYPES.get(k, k) for k in a.split(",")]
        return "is a " + " or ".join(kinds)
    if tok == "-size":
        return size_text(a)
    if tok in ("-mtime", "-atime", "-ctime"):
        return days_text(a, {"-mtime": "modified", "-atime": "accessed", "-ctime": "changed"}[tok], "days")
    if tok in ("-mmin", "-amin", "-cmin"):
        return days_text(a, {"-mmin": "modified", "-amin": "accessed", "-cmin": "changed"}[tok], "minutes")
    if tok in EXEC:
        end = args[-1] if args and args[-1] in (";", "+") else ";"
        body = args[:-1] if args and args[-1] in (";", "+") else args
        cmdline = " ".join("FILE" if x == "{}" else x for x in body)
        how = "once with MANY files at a time (fast)" if end == "+" else "once per file"
        where = " from inside the file's directory" if tok in ("-execdir", "-okdir") else ""
        ask = ", asking you first" if tok in ("-ok", "-okdir") else ""
        return f"run `{cmdline}` {how}{where}{ask}"
    if tok in ZERO_ARG:
        return ZERO_ARG[tok]
    return simple.get(tok, f"{tok} {' '.join(args)}".strip())


def parse_expr(tokens: list[str]):
    pos = 0
    warnings: list[str] = []

    def peek():
        return tokens[pos] if pos < len(tokens) else None

    def take():
        nonlocal pos
        pos += 1
        return tokens[pos - 1]

    def p_or():
        items = [p_and()]
        while peek() in ("-o", "-or"):
            take()
            items.append(p_and())
        return items[0] if len(items) == 1 else Node("or", items)

    def p_and():
        items = [p_not()]
        while peek() is not None and peek() not in ("-o", "-or", ")"):
            if peek() in ("-a", "-and"):
                take()
            items.append(p_not())
        return items[0] if len(items) == 1 else Node("and", items)

    def p_not():
        if peek() in ("!", "-not"):
            take()
            return Node("not", [p_not()])
        return p_primary()

    def p_primary():
        tok = take()
        if tok == "(":
            inner = p_or()
            if peek() == ")":
                take()
            else:
                warnings.append("a \\( is never closed with \\)")
            return Node("group", [inner])
        if tok in EXEC:
            args = []
            while peek() is not None:
                t = take()
                args.append(t)
                if t == ";" or (t == "+" and len(args) > 1 and args[-2] == "{}"):
                    break
            else:
                warnings.append(f"{tok} is missing its terminator: end it with \\; (or {{}} +)")
            if "{}" not in args:
                warnings.append(f"{tok} never mentions {{}}, so the command won't see the file names")
            return Node("leaf", text=primary_text(tok, args), action=True)
        args = []
        if tok in ONE_ARG and peek() is not None:
            args = [take()]
        elif tok == "-fprintf":
            args = [take(), take()] if pos + 1 < len(tokens) else []
        elif tok not in ZERO_ARG:
            warnings.append(f"unknown find primary {tok!r}")
        return Node("leaf", text=primary_text(tok, args), action=tok in ACTIONS)

    tree = p_or() if tokens else None
    if pos < len(tokens):
        warnings.append(f"unexpected {tokens[pos]!r} (unbalanced parentheses?)")
    return tree, warnings


def has_action(n: Node) -> bool:
    return n.action or any(has_action(c) for c in n.items)


def lint(n: Node, warnings: list[str]) -> None:
    if n.kind == "or":
        if any(has_action(b) for b in n.items) and not all(has_action(b) for b in n.items):
            warnings.append("an action sits inside only one side of -o, so it only runs for that side. "
                            "Wrap the tests: \\( -name a -o -name b \\) -exec …")
    if n.kind == "and":
        seen_delete = False
        for c in n.items:
            if c.kind == "leaf" and c.text == ZERO_ARG["-delete"]:
                seen_delete = True
            elif seen_delete and not c.action:
                warnings.append("-delete comes BEFORE a test: it deletes everything it reaches. Put -delete last.")
                break
    for c in n.items:
        lint(c, warnings)


def render(n: Node, depth: int = 0) -> list[str]:
    pad = "  " * depth
    if n.kind == "leaf":
        return [pad + ("⚠ " if n.text == ZERO_ARG["-delete"] else "") + n.text]
    if n.kind == "group":
        return render(n.items[0], depth)
    if n.kind == "not":
        inner = render(n.items[0], depth + 1)
        return [pad + "NOT:"] + inner
    word = "ALL of:" if n.kind == "and" else "ANY of:"
    out = [pad + word]
    for c in n.items:
        if c.kind == "group" and c.items[0].kind == n.kind:
            for g in c.items[0].items:
                out += render(g, depth + 1)
        else:
            out += render(c, depth + 1)
    return out


def explain(tokens: list[str]) -> str:
    if tokens and os.path.basename(tokens[0]) == "find":
        tokens = tokens[1:]
    while tokens and tokens[0] in ("-H", "-L", "-P"):
        tokens = tokens[1:]
    starts = []
    while tokens and not tokens[0].startswith("-") and tokens[0] not in ("(", "!", ")"):
        starts.append(tokens.pop(0))
    tree, warnings = parse_expr(tokens)
    lines = [f"Search in: {', '.join(starts) or '. (current directory)'}"]
    if tree is None:
        lines.append("Match: everything, and print each path")
    else:
        lint(tree, warnings)
        lines.append("For each file/directory found, keep going while:")
        lines += render(tree, 1)
        if not has_action(tree):
            lines.append("…and print the path of every match (the default action)")
    for w in dict.fromkeys(warnings):
        lines.append(f"⚠ {w}")
    return "\n".join(lines)


def register(sp):
    f = sp.add_parser("find", help="find(1) helpers: value suggestions and plain-English explanations")
    fs = f.add_subparsers(dest="find_cmd", required=True)
    e = fs.add_parser("explain", help="explain a find command line")
    e.add_argument("tokens", nargs="*")
    e.set_defaults(fn=lambda a: print(explain(a.tokens)))
    v = fs.add_parser("values", help="(used by the shell) suggestions for the argument of a find test")
    v.add_argument("prev")
    v.set_defaults(fn=lambda a: print("\n".join(f"{x}\t{d}" for x, d in (values_for(a.prev) or []))))
    o = fs.add_parser("operators", help="(used by the shell) find's operators with explanations")
    o.set_defaults(fn=lambda a: print("\n".join(f"{x}\t{d}" for x, d in operators())))
