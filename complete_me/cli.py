"""complete-me command line."""
from __future__ import annotations

import argparse
import json
import os
import sys
import time
from pathlib import Path

from . import __version__

CACHE = Path(os.environ.get("XDG_CACHE_HOME") or Path.home() / ".cache") / "complete-me"
MISS_TTL = 7 * 86400


def cache_dir(kind: str) -> Path:
    d = CACHE / kind
    d.mkdir(parents=True, exist_ok=True)
    return d


def atomic_write(path: Path, text: str) -> None:
    tmp = path.with_name(f".{path.name}.{os.getpid()}.tmp")
    tmp.write_text(text)
    os.replace(tmp, path)


def fish_has_completions(name: str) -> bool:
    """True if fish ships or has a completion file for name (excluding man-page guesses and ours)."""
    import shutil
    import subprocess
    if not shutil.which("fish"):
        return False
    script = ('status get-file completions/$argv[1].fish >/dev/null 2>&1; and exit 0; '
              'for d in $fish_complete_path; string match -q -- "*/generated_completions" $d; and continue; '
              'string match -q -- "*/complete-me/fish" $d; and continue; test -e $d/$argv[1].fish; and exit 0; end; exit 1')
    try:
        return subprocess.run(["fish", "--no-config", "-c", script, name], timeout=5,
                              stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL).returncode == 0
    except (OSError, subprocess.TimeoutExpired):
        return False


def cmd_gen(a) -> int:
    from . import probe, render
    rc = 0
    for name in a.commands:
        miss = cache_dir("miss") / name
        target = cache_dir("fish") / f"{name}.fish"
        if not a.force:
            if target.exists() and not a.quick:
                if time.time() - target.stat().st_mtime < MISS_TTL and "--quick" not in target.read_text()[:300]:
                    continue
            if miss.exists() and time.time() - miss.stat().st_mtime < MISS_TTL:
                continue
        if not a.force and fish_has_completions(name):
            if not a.quiet:
                print(f"{name}: fish already has completions for this; not shadowing them (use --force)", file=sys.stderr)
            continue
        if not probe.SAFE_NAME.match(name):
            print(f"complete-me: refusing unsafe name {name!r}", file=sys.stderr)
            rc = 1
            continue
        model = probe.gather(name, quick=a.quick)
        if model is None or not (model.options or model.subcommands):
            miss.touch()
            if not a.quiet:
                print(f"{name}: nothing found to complete", file=sys.stderr)
            rc = 1
            continue
        miss.unlink(missing_ok=True)
        text = render.fish(model)
        if a.quick:
            text = text.replace("# Regenerate:", "# --quick pass; full pass pending. Regenerate:", 1)
        atomic_write(target, text)
        atomic_write(cache_dir("bash") / name, render.bash(model))
        atomic_write(cache_dir("json") / f"{name}.json", json.dumps(model.to_dict(), indent=1))
        if not a.quiet:
            n_sub = sum(1 for p, _ in model.walk() if p)
            n_opt = sum(len(c.options) for _, c in model.walk())
            print(f"{name}: {n_opt} options, {n_sub} subcommands from {', '.join(model.sources)} -> {target}")
    return rc


def cmd_show(a) -> int:
    from . import probe
    model = probe.gather(a.command, quick=a.quick)
    if model is None:
        print(f"{a.command}: nothing found", file=sys.stderr)
        return 1
    if a.json:
        print(json.dumps(model.to_dict(), indent=1))
        return 0
    for path, node in model.walk():
        pad = "  " * len(path)
        print(f"{pad}{' '.join((model.name,) + path)}  {node.desc}")
        for o in node.options:
            arg = f" {o.arg}" if o.arg else ""
            vals = f"  [{', '.join(v for v, _ in o.values)}]" if o.values else ""
            kind = f"  ({o.kind})" if o.kind and o.kind != "enum" else ""
            print(f"{pad}    {', '.join(o.flags())}{arg}{vals}{kind}  {o.desc}")
    return 0


def cmd_forget(a) -> int:
    for name in a.commands:
        for p in (cache_dir("fish") / f"{name}.fish", cache_dir("bash") / name,
                  cache_dir("json") / f"{name}.json", cache_dir("miss") / name):
            p.unlink(missing_ok=True)
    return 0


def cmd_list(a) -> int:
    for p in sorted(cache_dir("fish").glob("*.fish")):
        print(p.stem)
    return 0


RG_ARGOPTS = {"-A", "-B", "-C", "-m", "-d", "-g", "-t", "-T", "-j", "-M", "-r", "-E", "--max-count", "--glob",
              "--iglob", "--type", "--type-not", "--threads", "--max-columns", "--replace", "--max-depth"}
GREP_ARGOPTS = {"-A", "-B", "-C", "-m", "-d", "-D", "--max-count", "--include", "--exclude", "--exclude-dir"}


def pattern_of(tokens: list[str]) -> str | None:
    tool = os.path.basename(tokens[0])
    rest = tokens[1:]
    if tool == "find":
        for i, t in enumerate(rest[:-1]):
            if t in ("-regex", "-iregex"):
                return rest[i + 1]
        return None
    if tool == "string":
        rest = rest[1:]
        argopts: set[str] = set()
    else:
        argopts = RG_ARGOPTS if tool == "rg" else GREP_ARGOPTS
    skip = False
    for i, t in enumerate(rest):
        if skip:
            skip = False
            continue
        if t in ("-e", "--regexp", "--expression") and i + 1 < len(rest):
            return rest[i + 1]
        if t.startswith("--regexp="):
            return t.split("=", 1)[1]
        if t.startswith("-"):
            skip = t in argopts
            continue
        return t
    return None


def cmd_explain(a) -> int:
    from . import findhelp, regex, probe
    toks = a.tokens
    if not toks:
        return 0
    tool = os.path.basename(toks[0])
    color = sys.stdout.isatty()
    if tool == "find":
        print(findhelp.explain(toks))
        pat = pattern_of(toks)
        if pat:
            print()
            print(regex.render_explain(pat, regex.detect(toks)[1], color))
        return 0
    if tool in ("grep", "egrep", "fgrep", "rgrep", "zgrep", "ugrep", "rg", "sed", "gsed", "string", "awk"):
        pat = pattern_of(toks)
        if pat is not None:
            print(regex.render_explain(pat, regex.detect(toks)[1], color))
            return 0
    model = probe.gather(tool, quick=True)
    if model is None:
        print(f"{tool}: no help available to explain with")
        return 1
    node, path = model, []
    for t in toks[1:]:
        if t.startswith("-"):
            flag = t.split("=", 1)[0]
            o = node.option(flag) or model.option(flag)
            if o is None and not t.startswith("--") and len(t) > 2:
                for ch in t[1:]:
                    oo = node.option("-" + ch) or model.option("-" + ch)
                    print(f"  -{ch:<22} {oo.desc if oo else '?'}")
                continue
            print(f"  {t:<23} {o.desc if o and o.desc else '?'}")
        elif t in node.subcommands:
            node = node.subcommands[t]
            path.append(t)
            print(f"  {t:<23} subcommand: {node.desc}")
        else:
            print(f"  {t:<23} argument")
    return 0


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(prog="complete-me", description="Completions for everything, safely.")
    ap.add_argument("--version", action="version", version=f"complete-me {__version__}")
    sp = ap.add_subparsers(dest="sub", required=True)

    g = sp.add_parser("gen", help="generate completions for commands that have none")
    g.add_argument("commands", nargs="+")
    g.add_argument("--force", action="store_true", help="regenerate even if cached")
    g.add_argument("--quick", action="store_true", help="top level only, fast (used on TAB)")
    g.add_argument("--quiet", action="store_true")
    g.set_defaults(fn=cmd_gen)

    s = sp.add_parser("show", help="print what complete-me understands about a command")
    s.add_argument("command")
    s.add_argument("--json", action="store_true")
    s.add_argument("--quick", action="store_true")
    s.set_defaults(fn=cmd_show)

    f = sp.add_parser("forget", help="delete generated completions for commands")
    f.add_argument("commands", nargs="+")
    f.set_defaults(fn=cmd_forget)

    sp.add_parser("list", help="list commands with generated completions").set_defaults(fn=cmd_list)

    e = sp.add_parser("explain", help="explain a command line: find expressions, regexes, flags")
    e.add_argument("tokens", nargs="*")
    e.set_defaults(fn=cmd_explain)

    from . import regex, findhelp, sshopts, snippets, aliases, bashfish
    regex.register(sp)
    findhelp.register(sp)
    sshopts.register(sp)
    snippets.register(sp)
    aliases.register(sp)
    bashfish.register(sp)

    a = ap.parse_args(argv)
    try:
        return a.fn(a) or 0
    except BrokenPipeError:
        return 0
    except KeyboardInterrupt:
        return 130
