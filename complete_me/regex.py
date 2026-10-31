"""Regex help: flavour detection, a TAB palette, plain-English explanations, a live builder.

Flavours
  bre    grep, sed (no -E): ( ) { } | + ? are LITERAL unless backslashed (GNU)
  ere    grep -E, egrep, sed -E, awk
  pcre   grep -P, rg -P / --pcre2, fish `string match -r`
  rust   rg default engine (no look-around, no back-references)
  emacs  find -regex default (GNU findutils)
  fixed  grep -F, rg -F: no regex at all
"""
from __future__ import annotations

import os
import re
import shlex
import shutil
import subprocess
import sys

FLAVOR_NAMES = {
    "bre": "basic regex (grep / sed default)",
    "ere": "extended regex (grep -E, sed -E, awk)",
    "pcre": "PCRE (grep -P, rg -P, fish string -r)",
    "rust": "Rust regex (rg default)",
    "emacs": "Emacs regex (find -regex default)",
    "python": "Python re",
    "fixed": "fixed string (no regex)",
}
MODERN = {"pcre", "rust", "python"}
GNU_POSIX = {"bre", "ere", "emacs"}


# ------------------------------------------------------------ flavour detection

def short_flags(tokens):
    for t in tokens:
        if t.startswith("-") and not t.startswith("--"):
            yield from t[1:]


def detect(tokens: list[str]) -> tuple[str, str]:
    """Return (tool, flavour) for a command line given as tokens."""
    if not tokens:
        return "", "ere"
    tool = os.path.basename(tokens[0])
    rest = tokens[1:]
    longs = {t.split("=")[0] for t in rest if t.startswith("--")}
    shorts = set(short_flags(t for t in rest if not t.startswith("--")))
    if tool == "string":
        return tool, "pcre"
    if tool in ("grep", "egrep", "fgrep", "rgrep", "zgrep", "ugrep", "ug"):
        if tool == "fgrep" or "F" in shorts or "--fixed-strings" in longs:
            return tool, "fixed"
        if "P" in shorts or "--perl-regexp" in longs:
            return tool, "pcre"
        if tool in ("egrep", "ugrep", "ug") or "E" in shorts or "--extended-regexp" in longs:
            return tool, "ere"
        return tool, "bre"
    if tool == "rg":
        if "F" in shorts or "--fixed-strings" in longs:
            return tool, "fixed"
        if "P" in shorts or "--pcre2" in longs or any(t.startswith("--engine=pcre2") for t in rest):
            return tool, "pcre"
        return tool, "rust"
    if tool in ("sed", "gsed"):
        return tool, "ere" if ("E" in shorts or "r" in shorts or "--regexp-extended" in longs) else "bre"
    if tool in ("awk", "gawk", "mawk"):
        return tool, "ere"
    if tool == "find":
        for i, t in enumerate(rest):
            if t == "-regextype" and i + 1 < len(rest):
                return tool, {"posix-extended": "ere", "posix-basic": "bre", "egrep": "ere",
                              "posix-egrep": "ere", "grep": "bre"}.get(rest[i + 1], "emacs")
        return tool, "emacs"
    if tool in ("python", "python3"):
        return tool, "python"
    return tool, "ere"


# ------------------------------------------------------------ explanation

ESCAPES = {
    "d": ("a digit 0-9", MODERN),
    "D": ("anything except a digit", MODERN),
    "w": ("a word character: letter, digit or _", MODERN | GNU_POSIX),
    "W": ("anything except a word character", MODERN | GNU_POSIX),
    "s": ("whitespace: space, tab, newline…", MODERN | GNU_POSIX),
    "S": ("anything except whitespace", MODERN | GNU_POSIX),
    "b": ("word boundary (edge between \\w and \\W), matches no characters", MODERN | GNU_POSIX),
    "B": ("NOT a word boundary", MODERN | GNU_POSIX),
    "<": ("start of a word (GNU)", GNU_POSIX),
    ">": ("end of a word (GNU)", GNU_POSIX),
    "A": ("start of the whole input", MODERN),
    "z": ("end of the whole input", MODERN),
    "Z": ("end of input (before a final newline)", {"pcre", "python"}),
    "K": ("forget everything matched so far: the match 'starts' here", {"pcre"}),
    "G": ("where the previous match ended", {"pcre"}),
    "n": ("newline", MODERN),
    "t": ("tab", MODERN),
    "r": ("carriage return", MODERN),
    "h": ("horizontal whitespace (space or tab)", {"pcre"}),
    "R": ("any line break (\\n, \\r\\n, …)", {"pcre"}),
    "`": ("start of the buffer (GNU)", GNU_POSIX),
    "'": ("end of the buffer (GNU)", GNU_POSIX),
}
SUGGEST = {
    "d": "use [0-9] or [[:digit:]], or switch to grep -P",
    "D": "use [^0-9]",
    "A": "use ^", "z": "use $", "n": "patterns are matched per line", "t": "use [[:blank:]]",
    "K": "only PCRE has \\K: grep -P or rg -P",
    "<": "use \\b", ">": "use \\b",
}
POSIX_CLASSES = {
    "alpha": "a letter", "digit": "a digit", "alnum": "a letter or digit", "upper": "an uppercase letter",
    "lower": "a lowercase letter", "space": "whitespace", "blank": "space or tab", "punct": "punctuation",
    "xdigit": "a hex digit", "cntrl": "a control character", "print": "a printable character",
    "graph": "a visible character", "word": "a word character",
}


def _special(flavor: str):
    """Which metacharacters are special bare vs. only when backslashed."""
    if flavor in ("bre", "emacs"):
        bare = set(".*^$[")
        escaped = set("(){}|+?") if flavor == "bre" else set("(){}|")
        if flavor == "emacs":
            bare |= set("+?")
        return bare, escaped
    return set(".*^$[(){}|+?"), set()


def describe_class(body: str, negated: bool) -> str:
    parts = []
    i = 0
    while i < len(body):
        m = re.match(r"\[:(\w+):\]", body[i:])
        if m:
            parts.append(POSIX_CLASSES.get(m.group(1), m.group(0)))
            i += m.end()
            continue
        if body[i] == "\\" and i + 1 < len(body):
            e = body[i + 1]
            parts.append(ESCAPES.get(e, (f"'{e}'",))[0])
            i += 2
            continue
        if i + 2 < len(body) and body[i + 1] == "-":
            parts.append(f"{body[i]}-{body[i + 2]}")
            i += 3
            continue
        parts.append(repr(body[i]) if body[i] != " " else "space")
        i += 1
    lead = "any ONE character EXCEPT" if negated else "any ONE of"
    return f"{lead}: {', '.join(parts)}"


def quantifier_text(q: str, lazy: bool) -> str:
    m = re.fullmatch(r"\{(\d*)(,?)(\d*)\}", q)
    if q == "*":
        t = "the previous item, 0 or more times"
    elif q == "+":
        t = "the previous item, 1 or more times"
    elif q == "?":
        t = "the previous item is optional (0 or 1 time)"
    elif m:
        lo, comma, hi = m.groups()
        if not comma:
            t = f"the previous item exactly {lo} times"
        elif not hi:
            t = f"the previous item {lo or 0} or more times"
        else:
            t = f"the previous item {lo or 0} to {hi} times"
    else:
        t = q
    return t + (" — as FEW as possible (lazy)" if lazy else " (as many as possible)")


def explain(pattern: str, flavor: str) -> list[tuple[int, str, str, str]]:
    """Return rows of (depth, token, meaning, warning)."""
    rows: list[tuple[int, str, str, str]] = []
    if flavor == "fixed":
        return [(0, pattern, "matched literally: this tool is in fixed-string mode (-F)", "")]
    bare, escaped = _special(flavor)
    depth = 0
    group_no = 0
    i = 0
    lit = ""

    def flush():
        nonlocal lit
        if lit:
            rows.append((depth, lit, f"the literal text '{lit}'", ""))
            lit = ""

    def add(tok, meaning, warn=""):
        flush()
        rows.append((depth, tok, meaning, warn))

    n = len(pattern)
    while i < n:
        c = pattern[i]
        nxt = pattern[i + 1] if i + 1 < n else ""
        # backslash forms
        if c == "\\" and nxt:
            if nxt in escaped:
                c, i = nxt, i + 2   # BRE \( etc: treat as the metachar
            elif nxt.isdigit() and nxt != "0":
                warn = "" if flavor not in ("rust",) else "back-references need rg -P"
                add("\\" + nxt, f"the same text that group {nxt} matched (back-reference)", warn)
                i += 2
                continue
            elif nxt in ESCAPES:
                meaning, ok = ESCAPES[nxt]
                warn = ""
                if flavor not in ok:
                    warn = f"not supported in {FLAVOR_NAMES[flavor]}"
                    if nxt in SUGGEST:
                        warn += f"; {SUGGEST[nxt]}"
                add("\\" + nxt, meaning, warn)
                i += 2
                continue
            elif nxt in "pP" and flavor in MODERN:
                m = re.match(r"\\[pP](\{[^}]*\}|\w)", pattern[i:])
                tok = m.group(0) if m else pattern[i:i + 2]
                add(tok, ("NOT " if nxt == "P" else "") + f"a character in Unicode class {tok[2:].strip('{}')}")
                i += len(tok)
                continue
            elif nxt == "x" and flavor in MODERN:
                m = re.match(r"\\x(\{[0-9a-fA-F]+\}|[0-9a-fA-F]{2})", pattern[i:])
                tok = m.group(0) if m else "\\x"
                add(tok, f"the character with hex code {tok[2:].strip('{}')}")
                i += len(tok)
                continue
            else:
                i += 2
                if nxt in ".*^$[]()|+?{}\\/":
                    add("\\" + nxt, f"a literal '{nxt}' (escaped)")
                else:
                    lit += nxt
                continue
        elif c in escaped and flavor in ("bre", "emacs"):
            add(c, f"a literal '{c}'", f"in {FLAVOR_NAMES[flavor]} a bare {c} is literal; write \\{c} or use -E")
            i += 1
            continue
        elif c not in bare and c not in "()|{}+?":
            lit += c
            i += 1
            continue
        elif c in "()|{}+?" and c not in bare:
            lit += c
            i += 1
            continue
        else:
            i += 1

        # c is an active metacharacter here (bare, or BRE-escaped)
        if c == ".":
            add(".", "any single character (except newline)")
        elif c == "^":
            add("^", "start of the line" if not rows and not lit else "start of line (only an anchor at the start)")
        elif c == "$":
            add("$", "end of the line")
        elif c == "[":
            j = i
            if j < n and pattern[j] == "^":
                j += 1
            if j < n and pattern[j] == "]":
                j += 1
            while j < n and pattern[j] != "]":
                if pattern.startswith("[:", j):
                    k = pattern.find(":]", j + 2)
                    j = k + 2 if k != -1 else j + 1
                    continue
                if pattern[j] == "\\" and flavor in MODERN:
                    j += 1
                j += 1
            body = pattern[i:j]
            negated = body.startswith("^")
            add("[" + body + "]", describe_class(body[1:] if negated else body, negated),
                "" if j < n else "unclosed [ — add ]")
            i = j + 1
        elif c in "*+?" or c == "{":
            tok = c
            if c == "{":
                m = re.match(r"\{\d*,?\d*\}" if flavor not in ("bre",) else r"\d*,?\d*\\\}", pattern[i - 1:] if flavor not in ("bre",) else pattern[i:])
                if flavor == "bre":
                    m2 = re.match(r"(\d*,?\d*)\\\}", pattern[i:])
                    if m2:
                        tok = "{" + m2.group(1) + "}"
                        i += m2.end()
                    else:
                        add("\\{", "a literal '{'")
                        continue
                else:
                    if m:
                        tok = m.group(0)
                        i += len(tok) - 1
                    else:
                        lit += "{"
                        continue
            lazy = flavor in MODERN and i < n and pattern[i] == "?"
            if lazy:
                i += 1
            poss = flavor == "pcre" and i < n and pattern[i] == "+"
            if poss:
                i += 1
            disp = ("\\" + tok if flavor == "bre" and c in "+?" else tok) + ("?" if lazy else "") + ("+" if poss else "")
            warn = "" if (rows or lit) else "nothing to repeat"
            if flavor in GNU_POSIX and i < n and pattern[i] == "?" and not lazy:
                warn = "lazy quantifiers (*?, +?) need PCRE: grep -P / rg"
            add(disp, quantifier_text(tok, lazy) + (" without backtracking (possessive)" if poss else ""), warn)
        elif c == "|":
            add("|" if flavor != "bre" else "\\|", "OR: either the part before or the part after")
        elif c == "(":
            m = re.match(r"\?(:|=|!|<=|<!|P?<(\w+)>|'(\w+)'|[imsxU-]+:|[imsxU-]+\))", pattern[i:])
            if m and flavor in MODERN:
                kind = m.group(1)
                i += m.end()
                if kind.endswith(")"):
                    add("(?" + kind, f"turn on flags {kind[:-1]} from here (i = ignore case, s = . matches newline, m = ^/$ per line, x = verbose)")
                    continue
                labels = {":": "group (not captured)", "=": "LOOKAHEAD: must be followed by …",
                          "!": "NEGATIVE LOOKAHEAD: must NOT be followed by …", "<=": "LOOKBEHIND: must be preceded by …",
                          "<!": "NEGATIVE LOOKBEHIND: must NOT be preceded by …"}
                warn = "look-around needs rg -P (PCRE2)" if flavor == "rust" and kind in ("=", "!", "<=", "<!") else ""
                if kind in labels:
                    add("(?" + kind, labels[kind], warn)
                elif kind.endswith(":"):
                    add("(?" + kind, f"group with flags {kind[:-1]} (i = ignore case)")
                else:
                    group_no += 1
                    add("(?" + kind, f"named capture group '{m.group(2) or m.group(3)}' (#{group_no})")
            elif m:
                add("(?" + m.group(1), "special group syntax", f"(?…) groups are not supported in {FLAVOR_NAMES[flavor]}")
                i += m.end()
            else:
                group_no += 1
                add("(" if flavor != "bre" else "\\(", f"start capture group #{group_no}")
            depth += 1
        elif c == ")":
            flush()
            depth = max(0, depth - 1)
            add(")" if flavor != "bre" else "\\)", "end of group")
        elif c == "}":
            lit += "}"
    flush()
    if depth > 0:
        rows.append((0, "", "", f"{depth} group(s) not closed"))
    return rows


def render_explain(pattern: str, flavor: str, color: bool) -> str:
    B, D, Y, R, C, X = ("\033[1m", "\033[2m", "\033[33m", "\033[31m", "\033[36m", "\033[0m") if color else ("",) * 6
    out = [f"{D}{FLAVOR_NAMES.get(flavor, flavor)}{X}", ""]
    if not pattern:
        out.append(f"{D}type a pattern…  TAB in the pattern slot shows every piece with its meaning{X}")
        return "\n".join(out)
    for depth, tok, meaning, warn in explain(pattern, flavor):
        pad = "  " * depth
        if tok:
            out.append(f"{pad}{C}{B}{tok:<10}{X} {meaning}")
        if warn:
            out.append(f"{pad}{'':10} {R}⚠ {warn}{X}")
    return "\n".join(out)


# ------------------------------------------------------------ palette & recipes

def palette(flavor: str) -> list[tuple[str, str]]:
    bs = flavor in ("bre",)
    e = (lambda s: "\\" + s) if bs else (lambda s: s)
    p = [
        (".", "any single character"),
        (".*", "anything (0 or more of any char)"),
        ("^", "start of line"),
        ("$", "end of line"),
        ("*", "previous item 0+ times"),
        (e("+"), "previous item 1+ times"),
        (e("?"), "previous item optional"),
        (e("{") + "2" + e("}"), "previous item exactly 2 times"),
        (e("{") + "2,5" + e("}"), "previous item 2 to 5 times"),
        (e("{") + "3," + e("}"), "previous item 3 or more times"),
        (e("("), "start a group  (close with " + e(")") + ")"),
        (e(")"), "end a group"),
        (e("|"), "OR: this or that"),
        ("[0-9]", "one digit"),
        ("[a-z]", "one lowercase letter"),
        ("[A-Za-z]", "one letter"),
        ("[^ ]", "one non-space character"),
        ("[[:space:]]", "one whitespace character"),
        ("[[:alnum:]_]", "one word character"),
        ("\\.", "a literal dot"),
    ]
    if flavor in MODERN:
        p += [("\\d", "a digit"), ("\\d+", "a number (1+ digits)"), ("\\w+", "a word"), ("\\s+", "whitespace"),
              ("\\b", "word boundary"), ("(?i)", "ignore case from here"), ("(?:", "group without capturing"),
              (".*?", "anything, as little as possible (lazy)")]
    if flavor in GNU_POSIX:
        p += [("\\w", "a word character (GNU)"), ("\\s", "whitespace (GNU)"), ("\\b", "word boundary (GNU)"),
              ("\\<", "start of word (GNU)"), ("\\>", "end of word (GNU)")]
    if flavor == "pcre":
        p += [("(?=", "lookahead: followed by…"), ("(?!", "negative lookahead: not followed by…"),
              ("(?<=", "lookbehind: preceded by…"), ("(?<!", "negative lookbehind"), ("\\K", "keep only what follows")]
    return p


RECIPES = [
    # name, ERE form, modern (PCRE/Rust) form
    ("IPv4 address", r"([0-9]{1,3}\.){3}[0-9]{1,3}", r"\b(?:\d{1,3}\.){3}\d{1,3}\b"),
    ("email address", r"[[:alnum:]._%+-]+@[[:alnum:].-]+\.[[:alpha:]]{2,}", r"[\w.%+-]+@[\w.-]+\.[A-Za-z]{2,}"),
    ("URL", r"https?://[^[:space:]\"'<>]+", r"https?://[^\s\"'<>]+"),
    ("ISO date 2026-10-30", r"[0-9]{4}-[0-9]{2}-[0-9]{2}", r"\b\d{4}-\d{2}-\d{2}\b"),
    ("time 23:59(:59)", r"[0-2][0-9]:[0-5][0-9](:[0-5][0-9])?", r"\b[0-2]\d:[0-5]\d(?::[0-5]\d)?\b"),
    ("UUID", r"[0-9a-fA-F]{8}-([0-9a-fA-F]{4}-){3}[0-9a-fA-F]{12}", r"\b[0-9a-fA-F]{8}(?:-[0-9a-fA-F]{4}){3}-[0-9a-fA-F]{12}\b"),
    ("hex colour #ff8800", r"#[0-9a-fA-F]{6}", r"#[0-9a-fA-F]{6}\b"),
    ("MAC address", r"([0-9a-fA-F]{2}:){5}[0-9a-fA-F]{2}", r"\b(?:[0-9a-fA-F]{2}:){5}[0-9a-fA-F]{2}\b"),
    ("semver 1.2.3", r"[0-9]+\.[0-9]+\.[0-9]+", r"\bv?\d+\.\d+\.\d+(?:-[\w.]+)?\b"),
    ("TODO / FIXME / XXX", r"TODO|FIXME|XXX|HACK", r"\b(?:TODO|FIXME|XXX|HACK)\b"),
    ("trailing whitespace", r"[[:space:]]+$", r"\s+$"),
    ("blank line", r"^[[:space:]]*$", r"^\s*$"),
    ("comment line (#)", r"^[[:space:]]*#", r"^\s*#"),
    ("number (int or decimal)", r"-?[0-9]+(\.[0-9]+)?", r"-?\d+(?:\.\d+)?"),
    ("hex number 0x1f", r"0x[0-9a-fA-F]+", r"\b0x[0-9a-fA-F]+\b"),
    ("double-quoted string", r"\"[^\"]*\"", r"\"(?:[^\"\\]|\\.)*\""),
    ("function definition", r"^[[:space:]]*(def|function|fn|func)[[:space:]]+[[:alnum:]_]+", r"^\s*(?:def|function|fn|func)\s+\w+"),
    ("whole word (edit WORD)", r"\bWORD\b", r"\bWORD\b"),
    ("line starting with WORD", r"^WORD", r"^WORD"),
]


def ere_to_bre(p: str) -> str:
    out, i = [], 0
    in_class = False
    while i < len(p):
        c = p[i]
        if c == "\\" and i + 1 < len(p):
            out.append(p[i:i + 2])
            i += 2
            continue
        if in_class:
            in_class = c != "]"
            out.append(c)
        elif c == "[":
            in_class = True
            out.append(c)
            if p[i + 1:i + 2] == "^":
                out.append("^"); i += 1
            if p[i + 1:i + 2] == "]":
                out.append("]"); i += 1
        elif c in "(){}|+?":
            out.append("\\" + c)
        else:
            out.append(c)
        i += 1
    return "".join(out)


def recipe_for(flavor: str, ere: str, modern: str) -> str:
    if flavor in MODERN:
        return modern
    if flavor == "bre":
        return ere_to_bre(ere)
    if flavor == "emacs":
        return ".*" + ere_to_bre(ere).replace("\\{", "{").replace("\\}", "}") + ".*"
    return ere


def candidates(tokens: list[str], current: str) -> list[tuple[str, str]]:
    tool, flavor = detect(tokens)
    if flavor == "fixed":
        return []
    raw = current
    for q in ("'", '"'):
        if raw.startswith(q):
            raw = raw[1:]
            break
    out = []
    if raw == "":
        for name, ere, modern in RECIPES:
            out.append((recipe_for(flavor, ere, modern), f"recipe: {name}"))
    trailing_escape = (len(raw) - len(raw.rstrip("\\"))) % 2 == 1
    base = raw[:-1] if trailing_escape else raw
    for tok, desc in palette(flavor):
        if raw and tok == "^" or trailing_escape and not tok.startswith("\\"):
            continue
        out.append((base + tok, desc))
    if trailing_escape:
        for ch in ".*+?()[]{}|^$/":
            out.append((base + "\\" + ch, f"a literal '{ch}'"))
    return out


# ------------------------------------------------------------ live builder

def search_command(tool: str, flavor: str, paths: list[str]) -> str:
    qp = " ".join(shlex.quote(p) for p in paths) or "."
    if shutil.which("rg") and tool in ("rg", "") or (tool == "rg"):
        eng = "-P " if flavor == "pcre" else ""
        return f"rg --color=always -n --no-heading --max-columns=200 -m 50 {eng}-e {{q}} -- {qp} 2>&1 | head -2000"
    if tool == "find":
        rt = {"ere": "posix-extended", "bre": "posix-basic"}.get(flavor, "emacs")
        return f"find {qp} -regextype {rt} -regex {{q}} 2>/dev/null | head -2000"
    if tool in ("sed", "awk", "string", "python"):
        mode = "-E" if flavor == "ere" else "-P" if flavor in ("pcre", "python") else ""
        return f"grep -rnI --color=always {mode} -e {{q}} -- {qp} 2>&1 | head -2000"
    mode = {"ere": "-E", "pcre": "-P", "bre": "-G"}.get(flavor, "-E")
    return f"grep -rnI --color=always {mode} -e {{q}} -- {qp} 2>&1 | head -2000"


def build(tokens: list[str], initial: str, paths: list[str]) -> int:
    tool, flavor = detect(tokens)
    if flavor == "fixed":
        flavor = "ere"
    self_cmd = f"{shlex.quote(sys.executable)} {shlex.quote(os.path.abspath(sys.argv[0]))}"
    if not shutil.which("fzf"):
        return fallback_builder(flavor, initial)
    search = search_command(tool, flavor, paths)
    fzf = [
        "fzf", "--disabled", "--ansi", "--print-query", "--query", initial,
        "--prompt", f"regex [{tool or 'grep'} · {flavor}] › ",
        "--header", "type to search live · ENTER puts the pattern on your command line · ESC cancels · ctrl-/ hides explain",
        "--bind", f"start:reload:[ -n {{q}} ] && {search} || true",
        "--bind", f"change:reload:sleep 0.08; [ -n {{q}} ] && {search} || true",
        "--bind", "ctrl-/:toggle-preview",
        "--preview", f"{self_cmd} regex explain --color --flavor {flavor} -- {{q}}",
        "--preview-window", "right,48%,wrap",
        "--height", "80%", "--layout", "reverse", "--border", "rounded",
    ]
    env = dict(os.environ, SHELL="/bin/sh")
    p = subprocess.run(fzf, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, env=env, text=True)
    if p.returncode not in (0, 1) or not p.stdout:
        return 130
    print(fish_quote(p.stdout.split("\n", 1)[0]))
    return 0


def fish_quote(s: str) -> str:
    """Single-quote for fish, doubling a backslash only where fish would otherwise eat it."""
    return "'" + re.sub(r"\\(?=[\\']|$)", r"\\\\", s).replace("'", "\\'") + "'"


def fallback_builder(flavor: str, initial: str) -> int:
    try:
        tin, tout = open("/dev/tty"), open("/dev/tty", "w")
    except OSError as e:
        print(f"complete-me: no terminal ({e}); install fzf for the live builder", file=sys.stderr)
        return 1
    saved = None
    try:
        import termios
        saved = termios.tcgetattr(tin)
        mode = termios.tcgetattr(tin)
        mode[3] |= termios.ECHO | termios.ICANON | termios.ISIG     # normal line editing while we prompt
        mode[0] |= termios.ICRNL
        mode[1] |= termios.OPOST | termios.ONLCR
        termios.tcsetattr(tin, termios.TCSANOW, mode)
    except (ImportError, OSError):
        pass
    try:
        return _prompt_loop(tin, tout, flavor, initial)
    finally:
        if saved is not None:
            termios.tcsetattr(tin, termios.TCSANOW, saved)


def _prompt_loop(tin, tout, flavor: str, initial: str) -> int:
    pat = initial
    tout.write("\n(fzf not found — simple mode. Install fzf for live matching: sudo pacman -S fzf)\n")
    try:
        while True:
            tout.write("\n" + render_explain(pat, flavor, True) + "\n\npattern (empty line = accept, ctrl-c = cancel) › ")
            tout.flush()
            line = tin.readline()
            if not line or line == "\n":
                break
            pat = line.rstrip("\n")
    except KeyboardInterrupt:
        return 130
    print(fish_quote(pat))
    return 0


# ------------------------------------------------------------ CLI

def register(sp):
    r = sp.add_parser("regex", help="regex palette, explanations and live builder")
    rs = r.add_subparsers(dest="regex_cmd", required=True)
    e = rs.add_parser("explain", help="explain a pattern piece by piece")
    e.add_argument("--flavor", choices=list(FLAVOR_NAMES), default=None)
    e.add_argument("--tool", help="detect the flavour from this command line instead", default="")
    e.add_argument("--color", action="store_true")
    e.add_argument("pattern", nargs="?", default="")
    e.set_defaults(fn=lambda a: print(render_explain(
        a.pattern, a.flavor or detect(shlex.split(a.tool))[1] if a.tool else a.flavor or "ere",
        a.color or sys.stdout.isatty())))
    c = rs.add_parser("complete", help="(used by the shell) TAB candidates for the pattern slot")
    c.add_argument("--current", default="")
    c.add_argument("tokens", nargs="*")
    c.set_defaults(fn=lambda a: print("\n".join(f"{v}\t{d}" for v, d in candidates(a.tokens, a.current))))
    b = rs.add_parser("build", help="live regex builder (fzf)")
    b.add_argument("--current", default="")
    b.add_argument("--path", action="append", default=[])
    b.add_argument("tokens", nargs="*")
    b.set_defaults(fn=lambda a: build(a.tokens, a.current.strip("'\""), a.path))
    d = rs.add_parser("flavor", help="print the regex flavour a command line uses")
    d.add_argument("tokens", nargs="*")
    d.set_defaults(fn=lambda a: print("\t".join(detect(a.tokens))))
