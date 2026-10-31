"""Gather help text for a command without letting it do anything.

Trust order: man page (pure text) > static script reading > `cmd [sub] --help`.
`--help` only ever runs inside a bubblewrap sandbox (read-only filesystem, no
network, private /tmp and /run so no D-Bus/agent/docker sockets, killed with us).  Without bwrap, binaries get `--help`
only if they are not on the deny list, and scripts are read, never run.
The target is never run bare, and never with -h or -?.
"""
from __future__ import annotations

import os
import re
import shutil
import signal
import subprocess
import time
from concurrent.futures import ThreadPoolExecutor

from . import scriptscan
from .model import Command
from .parse import clean, parse_help

NEVER_RUN = {
    "shutdown", "reboot", "poweroff", "halt", "init", "telinit", "kill", "killall", "pkill",
    "rm", "dd", "shred", "wipefs", "fdisk", "sfdisk", "parted", "mkfs", "mkswap", "sudo", "doas",
    "su", "pkexec", "login", "passwd", "yes", "cat", "tee", "sh", "bash", "zsh", "fish", "python",
    "python3", "node", "perl", "ruby", "vim", "nvim", "emacs", "nano", "less", "more", "top", "htop",
}
SAFE_NAME = re.compile(r"^[A-Za-z0-9_][\w.+-]*$")
MAX_SUBCOMMANDS = 80


def env_for_help() -> dict:
    keep = {k: v for k, v in os.environ.items() if k in ("PATH", "HOME", "LANG", "LC_ALL", "USER", "LOGNAME")}
    keep.update(TERM="dumb", NO_COLOR="1", PAGER="cat", MANPAGER="cat", GIT_PAGER="cat",
                COLUMNS="120", MANWIDTH="120", CLICOLOR="0")
    return keep


def sandbox_prefix() -> list[str] | None:
    bwrap = shutil.which("bwrap")
    if not bwrap or os.environ.get("COMPLETE_ME_SANDBOX") == "off":
        return None
    # Read-only blocks writes but not connect() on Unix sockets, so the socket directories
    # (/run: session D-Bus, systemd, docker.sock, agents; /tmp: X11, tmux) get private tmpfs.
    return [bwrap, "--ro-bind", "/", "/", "--dev", "/dev", "--proc", "/proc", "--tmpfs", "/tmp",
            "--tmpfs", "/run", "--unshare-all", "--die-with-parent", "--new-session", "--"]


def run_captured(argv: list[str], timeout: float) -> str:
    """Run with stdin=/dev/null in its own process group; kill the whole group on timeout."""
    try:
        p = subprocess.Popen(argv, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                             env=env_for_help(), start_new_session=True)
    except OSError:
        return ""
    try:
        out, _ = p.communicate(timeout=timeout)
    except subprocess.TimeoutExpired:
        try:
            os.killpg(p.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        out, _ = p.communicate()
    return out[:400_000].decode("utf-8", "replace")


def is_script(path: str) -> bool:
    try:
        with open(path, "rb") as f:
            return f.read(2) == b"#!"
    except OSError:
        return False


def man_text(name: str, timeout: float = 4) -> str:
    if not shutil.which("man") or not SAFE_NAME.match(name):
        return ""
    out = run_captured(["man", "--nh", "--nj", "-P", "cat", name], timeout)
    if not out.strip() or "No manual entry" in out[:200] or "unrecognized option" in out[:200]:
        out = run_captured(["man", "-P", "cat", name], timeout)  # BSD/macOS man
    if "No manual entry" in out[:200]:
        return ""
    return clean(out)


def help_text(argv: list[str], timeout: float) -> str:
    prefix = sandbox_prefix()
    if prefix is None:
        if os.path.basename(argv[0]) in NEVER_RUN or is_script(argv[0]):
            return ""
        return run_captured(argv + ["--help"], timeout)
    return run_captured(prefix + argv + ["--help"], timeout)


def echoes_parent(child: Command, parent: Command) -> bool:
    """Many CLIs print the top-level help for `cmd sub --help`; that tells us nothing about sub."""
    ps, cs = set(parent.subcommands), set(child.subcommands)
    if cs and ps and len(cs & ps) >= max(2, 0.8 * len(ps)):
        return True
    pf = {f for o in parent.options for f in o.flags()}
    cf = {f for o in child.options for f in o.flags()}
    return bool(cf) and len(cf & pf) >= 0.8 * len(cf) and len(cf) >= 4 and not cs


def looks_like_help(text: str) -> bool:
    return bool(re.search(r"usage|options|commands|^\s+-", text, re.I | re.M))


def path_dispatch(name: str, cmd: Command) -> bool:
    """git/clevis style: executables named NAME-sub[-subsub] on PATH are subcommands."""
    seen = False
    prefix = name + "-"
    names = set()
    for d in os.environ.get("PATH", "").split(os.pathsep):
        try:
            for e in os.scandir(d):
                if e.name.startswith(prefix) and SAFE_NAME.match(e.name) and os.access(e.path, os.X_OK):
                    names.add(e.name[len(prefix):])
        except OSError:
            continue
    for rest in sorted(names)[:300]:
        node = cmd
        for part in rest.split("-"):
            if not part:
                break
            node = node.subcommands.setdefault(part, Command(name=part, sources=["path"]))
        seen = True
    if seen and "path" not in cmd.sources:
        cmd.sources.append("path")
    return seen


MAX_DEPTH = 3


def probe_level(path: str, parent: Command, prefix: tuple[str, ...], parent_text: str,
                started: float, budget: float, depth: int) -> None:
    subs = list(parent.subcommands)[:MAX_SUBCOMMANDS]

    def probe(sub: str):
        if time.monotonic() - started > budget:
            return sub, ""
        return sub, help_text([path, *prefix, sub], 3)

    with ThreadPoolExecutor(max_workers=8) as pool:
        results = list(pool.map(probe, subs))
    for sub, text in results:
        if not text or not looks_like_help(text) or text == parent_text:
            continue
        parsed = parse_help(text, sub, "help")
        parsed.subcommands.pop(sub, None)
        if echoes_parent(parsed, parent):
            continue
        node = parent.subcommands[sub]
        node.options = node.options or parsed.options
        node.desc = node.desc or parsed.desc
        for n, s in parsed.subcommands.items():
            node.subcommands.setdefault(n, s)
        if parsed.subcommands:
            node.takes_files = parsed.takes_files
        if node.subcommands and depth < MAX_DEPTH:
            probe_level(path, node, prefix + (sub,), text, started, budget, depth + 1)


def gather(name: str, quick: bool = False, budget: float = 25.0) -> Command | None:
    if not SAFE_NAME.match(name):
        return None
    path = shutil.which(name)
    if not path:
        return None
    started = time.monotonic()
    cmd = Command(name=name)
    found = False

    if is_script(path):
        s = scriptscan.scan(path, name)
        if s:
            cmd.merge(s)
            found = True

    h = help_text([path], 2.5 if quick else 4)
    if h and looks_like_help(h):
        cmd.merge(parse_help(h, name, "help"))
        found = True

    m = man_text(name)
    if m:
        cmd.merge(parse_help(m, name, "man"))
        found = True

    found = path_dispatch(name, cmd) or found
    if not found:
        return None

    if not quick and sandbox_prefix() is not None and cmd.subcommands:
        probe_level(path, cmd, (), h, started, budget, depth=1)
    return cmd
