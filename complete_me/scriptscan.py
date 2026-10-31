"""Read shell and Python scripts for their interface without executing them."""
from __future__ import annotations

import ast
import re

from .model import Command, Option
from .parse import first_sentence, infer_kind, parse_help


def scan(path: str, name: str) -> Command | None:
    try:
        with open(path, "r", errors="replace") as f:
            src = f.read(500_000)
    except OSError:
        return None
    first = src.split("\n", 1)[0]
    if "python" in first:
        return scan_python(src, name)
    if re.search(r"\b(ba|z|k|da)?sh\b", first):
        return scan_shell(src, name)
    return None


# ---------------------------------------------------------------- shell

HEREDOC = re.compile(r"<<-?\s*['\"]?(\w+)['\"]?[^\n]*\n(.*?)\n\s*\1\s*$", re.S | re.M)
CASE_START = re.compile(r"^(?P<ind>[ \t]*)case\s+\"?\$\{?(?P<var>[\w@#:-]+)\}?\"?\s+in\b", re.M)


def usage_blocks(src: str, name: str) -> str:
    texts = []
    for m in HEREDOC.finditer(src):
        body = m.group(2)
        before = src[max(0, m.start() - 120):m.start()]
        if (re.search(r"usage|options|commands", body, re.I) or re.search(r"(usage|help)\w*\s*\(\)", before)
                or len(re.findall(rf"^\s+{re.escape(name)}\s", body, re.M)) >= 2):
            texts.append(body)
    for m in re.finditer(r"""(?:echo|printf)\s+(?:-e\s+)?(['"])((?:(?!\1).)*[Uu]sage(?:(?!\1).)*)\1""", src, re.S):
        texts.append(m.group(2).replace("\\n", "\n"))
    return "\n\n".join(texts)


def case_arms(src: str, start: int, base_indent: int) -> list[tuple[str, str]]:
    """Patterns of the case arms one level inside the case beginning at `start`."""
    arms = []
    lines = src[start:].split("\n")[1:]
    depth = 0
    arm_indent = None
    for ln in lines:
        s = ln.strip()
        if re.match(r"case\b", s):
            depth += 1
        if re.match(r"esac\b", s):
            if depth == 0:
                break
            depth -= 1
            continue
        if depth:
            continue
        m = re.match(r"^([ \t]*)\(?([\w|.*'\"-]+(?:\s*\|\s*[\w.*'\"-]+)*)\)(.*)$", ln)
        if not m:
            continue
        ind = len(m.group(1).expandtabs(8))
        if ind <= base_indent:
            continue
        if arm_indent is None:
            arm_indent = ind
        if ind != arm_indent:
            continue
        comment = re.search(r"(?:^|\s)#\s*(.+)$", m.group(3))
        arms.append((m.group(2), comment.group(1) if comment else ""))
    return arms


def scan_shell(src: str, name: str) -> Command:
    cmd = Command(name=name, sources=["script"])
    usage = usage_blocks(src, name)
    if usage:
        cmd.merge(parse_help(usage, name, "script"))

    for m in re.finditer(r"""getopts\s+['"]?:?([A-Za-z0-9:]+)['"]?""", src):
        spec = m.group(1)
        for i, ch in enumerate(spec):
            if ch != ":" and cmd.option("-" + ch) is None:
                takes = i + 1 < len(spec) and spec[i + 1] == ":"
                cmd.options.append(Option(shorts=[ch], arg="ARG" if takes else None))

    # The dispatcher is the outermost case on $1 / $cmd / $command / $action.
    dispatch = [m for m in CASE_START.finditer(src)
                if m.group("var").split(":")[0] in ("1", "cmd", "command", "action", "sub", "subcmd", "verb", "op")]
    dispatch.sort(key=lambda m: len(m.group("ind").expandtabs(8)))
    if dispatch:
        top = len(dispatch[0].group("ind").expandtabs(8))
        for m in (d for d in dispatch if len(d.group("ind").expandtabs(8)) == top):
            for pattern, comment in case_arms(src, m.start(), top):
                for alt in (p.strip().strip("'\"") for p in pattern.split("|")):
                    if alt.startswith("-"):
                        flag = alt
                        if re.fullmatch(r"--?[\w-]+", flag) and cmd.option(flag) is None:
                            o = Option()
                            (o.longs if flag.startswith("--") else o.shorts).append(flag.lstrip("-"))
                            cmd.options.append(o)
                    elif re.fullmatch(r"[A-Za-z0-9][\w.-]*", alt) and alt not in cmd.subcommands:
                        cmd.subcommands[alt] = Command(name=alt, desc=first_sentence(comment), sources=["script"])
    if cmd.subcommands and not re.search(r"\b(FILE|PATH)\b|<file", usage):
        cmd.takes_files = False
    return cmd


# ---------------------------------------------------------------- python

def _const(node):
    return node.value if isinstance(node, ast.Constant) else None


def scan_python(src: str, name: str) -> Command | None:
    try:
        tree = ast.parse(src)
    except SyntaxError:
        return None
    cmd = Command(name=name, sources=["script"])
    parsers: dict[str, Command] = {}   # variable name -> node

    def target_name(call: ast.Call) -> str | None:
        f = call.func
        if isinstance(f, ast.Attribute) and isinstance(f.value, ast.Name):
            return f.value.id
        return None

    for node in ast.walk(tree):
        if isinstance(node, ast.Assign) and isinstance(node.value, ast.Call):
            call = node.value
            f = call.func
            if isinstance(f, ast.Attribute) and f.attr == "add_parser" and call.args:
                sub = _const(call.args[0])
                if isinstance(sub, str):
                    kw = {k.arg: _const(k.value) for k in call.keywords}
                    node_cmd = Command(name=sub, desc=first_sentence(kw.get("help") or ""), sources=["script"])
                    cmd.subcommands[sub] = node_cmd
                    for t in node.targets:
                        if isinstance(t, ast.Name):
                            parsers[t.id] = node_cmd

    for node in ast.walk(tree):
        if not (isinstance(node, ast.Call) and isinstance(node.func, ast.Attribute)):
            continue
        f = node.func
        if f.attr == "add_parser" and node.args and isinstance(_const(node.args[0]), str):
            sub = _const(node.args[0])
            if sub not in cmd.subcommands:
                kw = {k.arg: _const(k.value) for k in node.keywords}
                cmd.subcommands[sub] = Command(name=sub, desc=first_sentence(kw.get("help") or ""), sources=["script"])
        if f.attr != "add_argument":
            continue
        flags = [a for a in (_const(x) for x in node.args) if isinstance(a, str) and a.startswith("-")]
        if not flags:
            continue
        o = Option()
        for fl in flags:
            if fl.startswith("--"):
                o.longs.append(fl[2:])
            elif len(fl) == 2:
                o.shorts.append(fl[1:])
            else:
                o.olds.append(fl[1:])
        for k in node.keywords:
            if k.arg == "help" and isinstance(_const(k.value), str):
                o.desc = first_sentence(_const(k.value))
            elif k.arg == "choices" and isinstance(k.value, (ast.List, ast.Tuple)):
                o.values = [(str(_const(e)), "") for e in k.value.elts if _const(e) is not None]
            elif k.arg == "metavar" and isinstance(_const(k.value), str):
                o.arg = _const(k.value)
            elif k.arg == "action" and _const(k.value) in ("store_true", "store_false", "count", "help", "version"):
                o.arg = None
                break
        else:
            o.arg = o.arg or (o.longs[0].upper() if o.longs else "ARG")
        o.kind = infer_kind(o)
        owner = parsers.get(target_name(node) or "", cmd)
        if owner.option(o.flags()[0]) is None:
            owner.options.append(o)

    # sys.argv[1] == "word" dispatch
    for m in re.finditer(r"""sys\.argv\[1\]\s*==\s*['"]([\w.-]+)['"]""", src):
        cmd.subcommands.setdefault(m.group(1), Command(name=m.group(1), sources=["script"]))
    if not cmd.options and not cmd.subcommands:
        return None
    return cmd
