"""Turn --help output and man pages into a Command model.

Everything here is pure text processing: no subprocesses, no shell.
"""
from __future__ import annotations

import re

from .model import Command, Option

ANSI = re.compile(r"\x1b\[[0-9;?]*[A-Za-z]|\x1b\][^\x07]*\x07")
OVERSTRIKE = re.compile(r".\x08")


def clean(text: str) -> str:
    text = ANSI.sub("", text)
    text = OVERSTRIKE.sub("", text)
    return text.replace("\t", "        ").replace("\r", "")


def indent_of(line: str) -> int:
    return len(line) - len(line.lstrip(" "))


# ---------------------------------------------------------------- sections

NON_OPTION_SECTIONS = ("example", "see also", "environment", "files", "exit status", "notes",
                       "bugs", "author", "copyright", "reporting", "history", "standards")
NOT_COMMAND_WORDS = ("option", "flag", "switch", "argument", "example", "environment", "setting",
                     "variable", "see also", "usage", "synopsis")


def header_of(line: str) -> str | None:
    stripped = line.rstrip()
    t = stripped.strip()
    if not t:
        return None
    ind = indent_of(stripped)
    if ind <= 1 and t.endswith(":") and len(t) < 70 and not t.startswith("-"):
        return t[:-1].lower()
    if ind == 0 and re.fullmatch(r"[A-Z][A-Z0-9 /_&-]{2,}", t):
        return t.lower()
    return None


def is_command_header(h: str) -> bool:
    return bool(re.search(r"\b(sub)?commands?\b", h)) and not any(w in h for w in NOT_COMMAND_WORDS[:4])


# ---------------------------------------------------------------- options

FLAG = re.compile(r"(--?[A-Za-z0-9?#][\w.#+?-]*)")
ARG_FORMS = [
    re.compile(r"\[=(?P<arg>[^\]]+)\]"),                     # --color[=WHEN]
    re.compile(r"=(?P<arg><[^>]+>|[^\s,\]]+)"),              # --file=FILE
    re.compile(r"\[(?P<arg><[^>]+>|[A-Z][\w-]*)\]"),         # -x[ARG]
    re.compile(r"(?P<arg><[^>]+>(?:\.\.\.)?)"),              # -D<path>
    re.compile(r"[ ](?! )(?P<arg><[^>]+>(?:\.\.\.)?|\{[^}]+\}|\[[^\]]+\]|[A-Z][A-Z0-9_|:-]*(?:\.\.\.)?)"),
    re.compile(r"[ ](?! )(?P<arg>[a-z][\w-]*)(?=\s*,\s*-|\s{2,}|\s*$)"),
]
SEPARATOR = re.compile(r"\s*,\s*|\s*\|\s*|\s+or\s+|[ ](?=-)")


def parse_option_spec(line: str):
    """Return (Option, rest_of_line) if the line begins an option entry."""
    body = line.lstrip(" ")
    if not body.startswith("-") or body.startswith("---"):
        return None
    pos = 0
    opt = Option()
    while True:
        m = FLAG.match(body, pos)
        if not m:
            break
        flag = m.group(1).rstrip(".")
        pos = m.end()
        if body.startswith("...", pos):
            pos += 3
        if flag in ("-", "--"):
            return None
        if flag.startswith("--"):
            opt.longs.append(flag[2:])
        elif len(flag) == 2:
            opt.shorts.append(flag[1:])
        else:
            opt.olds.append(flag[1:])
        for form in ARG_FORMS:
            a = form.match(body, pos)
            if a:
                opt.arg = a.group("arg")
                opt.arg_optional = body[pos:pos + 2] == "[=" or body[pos] == "["
                pos = a.end()
                break
        s = SEPARATOR.match(body, pos)
        if s and body[s.end():s.end() + 1] == "-":
            pos = s.end()
            continue
        break
    if not (opt.shorts or opt.longs or opt.olds):
        return None
    rest = body[pos:]
    if rest and not re.match(r"\s{2,}|\s+-\s+|\s*$", rest):
        return None  # prose that merely starts with a dash
    return opt, re.sub(r"^\s*-?\s*", "", rest).rstrip() if rest.strip() else ""


def first_sentence(text: str, limit: int = 90) -> str:
    text = " ".join(text.split())
    m = re.match(r"(.+?[.!?])(\s|$)", text)
    s = m.group(1) if m else text
    if len(s) > limit:
        s = s[: limit - 1].rsplit(" ", 1)[0] + "…"
    return s.rstrip(".")


# ---------------------------------------------------------------- values

QUOTED = re.compile(r"""['"`‘“]([\w.@+:-]{1,30})['"`’”]""")
VALUE_PHRASE = re.compile(
    r"(?:possible values(?: for this \w+)?(?: are)?|valid (?:values|arguments|options)(?: are)?|accepted values(?: are)?|"
    r"one of|must be(?: one of)?|may be(?: one of)?|can be(?: one of)?|choices(?: are)?|either|supported values(?: are)?)"
    r"\s*:?\s*(?P<tail>.{0,300})", re.I | re.S)
WORD_VALUE = re.compile(r"^[\w.+@-]{1,30}$")


def split_value_list(chunk: str) -> list[str]:
    chunk = re.sub(r"\([^)]*\)", "", chunk)                  # "(the default)"
    chunk = re.split(r"[.;\]\n]|\s{2,}", chunk, maxsplit=1)[0]
    parts = [p.strip(" '\"`") for p in re.split(r",|\bor\b|\band\b|\|", chunk)]
    parts = [p for p in parts if p]
    if len(parts) >= 2 and all(WORD_VALUE.match(p) for p in parts):
        return parts
    return []


def extract_values(opt: Option, full: str) -> list[tuple[str, str]]:
    arg = opt.arg or ""
    inner = arg.strip("<>[]{}")
    if (arg.startswith("{") or "|" in arg) and inner:
        vals = [v for v in re.split(r"[,|]", inner) if WORD_VALUE.match(v)]
        if len(vals) >= 2:
            return [(v, "") for v in vals]
    # "word: description" lines following a "possible values" sentence (rg, clap long help)
    m = re.search(r"(?:possible values|values are|one of the following)[^\n]*:\s*\n(?P<rest>(?:.*\n?){1,60})", full, re.I)
    if m:
        pairs = re.findall(r"^\s*(?:-\s+)?([\w.+-]{1,30}):\s+(\S.*)$", m.group("rest"), re.M)
        if len(pairs) >= 2:
            return [(v, first_sentence(d, 60)) for v, d in pairs]
    m = re.search(r"\[possible values: ([^\]]+)\]", full)
    if m:
        vals = split_value_list(m.group(1))
        if vals:
            return [(v, "") for v in vals]
    for m in VALUE_PHRASE.finditer(full):
        tail = m.group("tail")
        sentence = re.split(r"(?<=[.;])\s", tail, maxsplit=1)[0]
        quoted = QUOTED.findall(sentence)
        if len(quoted) >= 2:
            return [(v, "") for v in dict.fromkeys(quoted)]
        vals = split_value_list(tail)
        if vals:
            return [(v, "") for v in vals]
    # docker: Set the logging level ("debug", "info", "warn")
    m = re.search(r"""\(((?:\s*["'][\w.-]+["'],?){3,})\s*\)""", full)
    if m:
        return [(v, "") for v in dict.fromkeys(QUOTED.findall(m.group(1)))]
    # ls --sort=WORD: "none (-U), size (-S), time (-t)"
    pairs = list(dict.fromkeys(re.findall(r"\b([a-z][\w-]*) \((?:--?[\w-]+|default)\)", full)))
    if len(pairs) >= 2:
        return [(v, "") for v in dict.fromkeys(pairs)]
    # ls --quoting-style=WORD: "...entry names: literal, locale, shell"
    m = re.search(r":\s*((?:[a-z][\w-]*,\s*){2,}[a-z][\w-]*)\b", full)
    if m and inner and inner.isupper():
        vals = split_value_list(m.group(1))
        if vals:
            return [(v, "") for v in vals]
    return []


def values_from_placeholder(placeholder: str, text: str) -> list[tuple[str, str]]:
    """GNU style: 'The WHEN argument defaults to 'always' and can also be 'auto' or 'never'."""
    word = placeholder.strip("<>[]=.")
    if not re.fullmatch(r"[A-Z][A-Z_]{2,}", word):
        return []
    for sentence in re.split(r"(?<=[.])\s+", " ".join(text.split())):
        if re.search(rf"\b{word}\b (?:argument |value )?(?:is|can|may|must|defaults)", sentence):
            q = QUOTED.findall(sentence)
            if len(q) >= 2:
                return [(v, "") for v in dict.fromkeys(q)]
    return []


KINDS = [
    ("dir", r"\b(dir|directory|dirs|directories|folder)\b"),
    ("file", r"\b(file|files|filename|path|paths|manifest|config|output|input|socket|log_file|script)\b|_file\b|\.\w+$"),
    ("user", r"\b(user|username|login)\b"),
    ("group", r"\bgroup\b"),
    ("host", r"\b(host|hostname|address|addr)\b"),
    ("pid", r"\bpid\b"),
    ("command", r"\b(command|cmd|program)\b"),
    ("number", r"\b(n|num|number|int|count|size|seconds|secs|ms|port|level|depth|bytes|lines|jobs|width)\b"),
]


def infer_kind(opt: Option) -> str:
    if opt.values:
        return "enum"
    if not opt.arg:
        return ""
    a = opt.arg.strip("<>[]=.").replace("-", "_").lower()
    for kind, pat in KINDS:
        if re.search(pat, a):
            return kind
    return ""


# ---------------------------------------------------------------- commands

CMD_ENTRY = re.compile(
    r"^(?P<ind>\s{1,10})(?P<names>[A-Za-z0-9][\w.:+-]*(?:\(\d\w*\))?\*?(?:\s*,\s*[A-Za-z0-9][\w.:+-]*)*)"
    r"(?P<args>(?:\s+(?:<[^>]*>|\[[^\]]*\]|[A-Z][A-Z0-9_.|,-]*(?:\.\.\.|…)?|\{[^}]*\}))*)"
    r"(?:\s{2,}|\s+-\s+)(?:-\s+)?(?P<desc>\S.*)$")
CMD_ALONE = re.compile(
    r"^(?P<ind>\s{1,10})(?P<names>[A-Za-z0-9][\w.:+-]*(?:\(\d\w*\))?(?:\s*,\s*[A-Za-z0-9][\w.:+-]*)*)"
    r"(?P<args>(?:\s+(?:<[^>]*>|\[[^\]]*\]|[A-Z][A-Z0-9_.|,-]*(?:\.\.\.|…)?|\{[^}]*\}))*)\s*$")
BRACES = re.compile(r"^\s+\{([\w,.-]+)\}\s*")


def normalise_cmd_name(raw: str, parent: str) -> str:
    n = re.sub(r"\(\d\w*\)$", "", raw.rstrip("*"))
    if n.startswith(parent + "-"):
        n = n[len(parent) + 1:]
    return n


USAGE_FILE = re.compile(r"\b(FILE|PATH|DIR|DIRECTORY|SOURCE|DEST|TARGET|FILENAME)S?\b|<(file|path|dir)[\w-]*>|\bfile\b", re.I)


def parse_help(text: str, name: str, source: str = "help") -> Command:
    text = clean(text)
    lines = text.split("\n")
    cmd = Command(name=name, sources=[source])
    section = ""
    in_cmds = False
    usage_lines: list[str] = []
    in_usage = False
    i = 0
    while i < len(lines):
        line = lines[i].rstrip()
        h = header_of(line)
        um = re.match(r"^\s*usage:\s*(.*)$", line, re.I)
        if um:
            in_usage = True
            usage_lines.append(um.group(1))
            i += 1
            continue
        if in_usage:
            if line.strip() and not h:
                usage_lines.append(line)
                i += 1
                continue
            in_usage = False
        if h is not None:
            section = h
            in_cmds = is_command_header(h)
            if h in ("synopsis",):
                in_usage = True
            i += 1
            continue
        if not line.strip():
            i += 1
            continue
        if any(section.startswith(s) for s in NON_OPTION_SECTIONS):
            i += 1
            continue

        spec = parse_option_spec(line)
        if spec:
            opt, inline = spec
            ind = indent_of(line)
            desc_lines = [inline] if inline else []
            desc_col = len(line) - len(inline) if inline else ind + 3
            j = i + 1
            while j < len(lines):
                nxt = lines[j].rstrip()
                if not nxt.strip():
                    k = j + 1
                    while k < len(lines) and not lines[k].strip():
                        k += 1
                    if k < len(lines) and indent_of(lines[k]) > ind + 2 and not header_of(lines[k]):
                        desc_lines.append("")
                        j = k
                        continue
                    break
                nind = indent_of(nxt)
                if nind <= ind or header_of(nxt):
                    break
                if nxt.lstrip().startswith("-") and nind < desc_col and parse_option_spec(nxt):
                    break
                if not desc_lines:
                    desc_col = nind
                desc_lines.append(nxt.strip())
                j += 1
            full = "\n".join(desc_lines)
            opt.desc = first_sentence(full.split("\n\n")[0])
            raw_full = "\n".join(lines[i + 1:j]) if not inline else inline + "\n" + "\n".join(lines[i + 1:j])
            opt.values = extract_values(opt, raw_full)
            if not opt.values and opt.arg:
                opt.values = values_from_placeholder(opt.arg, text)
            if opt.arg and not opt.values and opt.arg.startswith("[") and "|" not in opt.arg:
                pass
            opt.kind = infer_kind(opt)
            existing = cmd.option(opt.flags()[0])
            if existing is None:
                cmd.options.append(opt)
            elif not existing.desc:
                existing.desc = opt.desc
            i = j
            continue

        b = BRACES.match(line)
        if b and section.startswith(("positional", "commands", "subcommands", "available")):
            for n in b.group(1).split(","):
                cmd.subcommands.setdefault(n, Command(name=n, sources=[source]))
            in_cmds = True
            i += 1
            continue

        self_named = re.match(rf"^(\s+){re.escape(name)}\s+(?=[A-Za-z0-9])", line)
        if self_named:
            # "  party up <roster.tsv>    launch every agent" (script usage text)
            m = CMD_ENTRY.match(self_named.group(1) + line[self_named.end():])
            if m:
                primary = normalise_cmd_name(m.group("names").split(",")[0].strip(), name)
                if re.fullmatch(r"\w[\w.:+-]*", primary):
                    sub = cmd.subcommands.get(primary) or Command(name=primary, sources=[source])
                    sub.desc = sub.desc or first_sentence(m.group("desc"))
                    sub.takes_files = bool(USAGE_FILE.search(m.group("args")) or re.search(r"<[^>]*\.\w+>", m.group("args")))
                    cmd.subcommands[primary] = sub
                    i += 1
                    continue

        orphan = False
        if not in_cmds and section in ("", "usage") and not cmd.subcommands_locked:
            # GLib-style: a bare table of commands with no header. Needs 3 entries in a row.
            run = 0
            for k in range(i, min(i + 12, len(lines))):
                if CMD_ENTRY.match(lines[k]) or (CMD_ALONE.match(lines[k]) and k + 1 < len(lines)
                                                 and indent_of(lines[k + 1]) > indent_of(lines[k]) + 8):
                    run += 1
                elif lines[k].strip() and indent_of(lines[k]) <= indent_of(line):
                    break
            orphan = run >= 3
            in_cmds = orphan
        if in_cmds:
            m = CMD_ENTRY.match(line)
            desc = ""
            if m:
                desc = m.group("desc")
            else:
                m = CMD_ALONE.match(line)
                if m:
                    k = i + 1
                    if k < len(lines) and indent_of(lines[k]) > indent_of(line) and lines[k].strip() \
                            and not lines[k].lstrip().startswith("-"):
                        desc = lines[k].strip()
                        i = k
                    else:
                        m = None
            if m:
                names = [normalise_cmd_name(n.strip(), name) for n in m.group("names").split(",")]
                primary = names[0]
                if primary and not primary.endswith(":") and re.fullmatch(r"\w[\w.:+-]*", primary):
                    sub = cmd.subcommands.get(primary) or Command(name=primary, sources=[source])
                    sub.desc = sub.desc or first_sentence(desc.lstrip("- "))
                    sub.aliases = [a for a in names[1:] if a and a != primary]
                    args = m.group("args").strip()
                    if args:
                        sub.takes_files = bool(USAGE_FILE.search(args) or re.search(r"\.\w+>", args)
                                               or re.search(r"\b[A-Z]*(FILE|ARCHIVE|PATH|DIR)[A-Z]*\b", args))
                    cmd.subcommands[primary] = sub
        i += 1

    usage = "\n".join(usage_lines)
    for m in re.finditer(r"(?<![\w<-])(--[A-Za-z][\w-]*|-[A-Za-z0-9?])(?=[\s\]|=,<[]|$)(\[?=(<[^>]+>|[A-Z][\w-]*)\]?| (<[^>]+>|[A-Z][A-Z_]+))?", usage):
        flag = m.group(1)
        if cmd.option(flag) is None:
            o = Option(arg=m.group(3) or m.group(4))
            (o.longs if flag.startswith("--") else o.shorts).append(flag.lstrip("-"))
            o.kind = infer_kind(o)
            cmd.options.append(o)
    if cmd.subcommands:
        cmd.takes_files = bool(USAGE_FILE.search(usage))
    return cmd
