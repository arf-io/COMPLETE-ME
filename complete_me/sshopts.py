"""ssh -oKey=Value: every ssh_config keyword with its meaning and its allowed values.

Parsed from ssh_config(5) once and cached; algorithm lists come from `ssh -Q`.
"""
from __future__ import annotations

import glob
import json
import os
import re
import shutil
import time
from pathlib import Path

from .parse import first_sentence, split_value_list
from .probe import man_text, run_captured

CACHE = Path(os.environ.get("XDG_CACHE_HOME") or Path.home() / ".cache") / "complete-me" / "data" / "ssh_config.json"
QUERY = {"ciphers": "cipher", "macs": "mac", "kexalgorithms": "kex", "hostkeyalgorithms": "key-sig",
         "pubkeyacceptedalgorithms": "key-sig", "hostbasedacceptedalgorithms": "key-sig",
         "casignaturealgorithms": "sig"}
FILES = {"identityfile", "certificatefile", "userknownhostsfile", "globalknownhostsfile", "controlpath",
         "identityagent", "securitykeyprovider", "pkcs11provider", "revokedhostkeys", "include", "knownhostscommand"}
HOSTS = {"proxyjump", "hostname", "hostkeyalias"}
STOP = {"the", "a", "an", "be", "this", "that", "it", "is", "to", "not", "set", "used", "any", "all", "none."}


def parse_ssh_config_man(text: str) -> dict:
    keys: dict[str, dict] = {}
    lines = text.split("\n")
    i = 0
    while i < len(lines):
        m = re.match(r"^ {5}([A-Z][A-Za-z0-9]+)\s*$", lines[i])
        if not m:
            i += 1
            continue
        name = m.group(1)
        j = i + 1
        body = []
        while j < len(lines) and (not lines[j].strip() or lines[j].startswith("      ")):
            body.append(lines[j].strip())
            j += 1
        full = " ".join(" ".join(body).split())
        if not full:   # several keywords sharing one description
            i = j
            continue
        vals: list[str] = []
        m2 = re.search(r"possible values are:?\s*(.+?)\.(?:\s|$)", full, re.I)
        if m2:
            vals = split_value_list(m2.group(1).replace(" and ", ", "))
        if not vals:
            m2 = re.search(r"(?:argument[s]? (?:to this keyword )?must be|may be one of|valid arguments are|accepted values are)\s*:?\s*(.+?)\.(?:\s|$)", full, re.I)
            if m2:
                vals = split_value_list(m2.group(1))
        set_to = re.findall(r"(?:set to|setting this to|set this to|is set to|flag is set to)\s+\"?([\w-]+)\"?(?:\s+or\s+\"?([\w-]+))?", full, re.I)
        for a, b in set_to:
            for v in (a, b):
                if v and v.lower() not in STOP and v not in vals:
                    vals.append(v)
        if not vals and re.match(r"(Specifies|Indicates) whether", full):
            vals = ["yes", "no"]
        if vals == ["yes"]:
            vals = ["yes", "no"]
        keys[name] = {"desc": first_sentence(full, 80), "values": vals}
        i = j
    return keys


def load() -> dict:
    try:
        if time.time() - CACHE.stat().st_mtime < 30 * 86400:
            return json.loads(CACHE.read_text())
    except (OSError, ValueError):
        pass
    keys = parse_ssh_config_man(man_text("ssh_config"))
    if shutil.which("ssh"):
        for k in list(keys):
            q = QUERY.get(k.lower())
            if q:
                algos = run_captured(["ssh", "-Q", q], 3).split()
                if algos:
                    keys[k]["values"] = algos
    CACHE.parent.mkdir(parents=True, exist_ok=True)
    CACHE.write_text(json.dumps(keys))
    return keys


def candidates(current: str) -> list[tuple[str, str]]:
    keys = load()
    prefix = ""   # fish re-attaches "-o" itself for the -oKey=Value form
    cur = current[2:] if current.startswith("-o") else current
    if "=" not in cur:
        return [(f"{prefix}{k}=", v["desc"]) for k, v in sorted(keys.items())]
    key, val = cur.split("=", 1)
    real = next((k for k in keys if k.lower() == key.lower()), None)
    if real is None:
        return []
    out = [(f"{prefix}{real}={v}", "") for v in keys[real]["values"]]
    lk = real.lower()
    if lk in FILES:
        base = os.path.expanduser(val) if val else os.path.expanduser("~/.ssh/")
        for p in sorted(glob.glob(base + "*"))[:200]:
            shown = p.replace(os.path.expanduser("~"), "~", 1) if not val.startswith("/") else p
            out.append((f"{prefix}{real}={shown}{'/' if os.path.isdir(p) else ''}", "file"))
    if lk in HOSTS:
        for h in known_hosts():
            out.append((f"{prefix}{real}={h}", "host from ~/.ssh/config"))
    return out


def known_hosts() -> list[str]:
    hosts = []
    try:
        for line in open(os.path.expanduser("~/.ssh/config"), errors="replace"):
            m = re.match(r"\s*Host\s+(.+)", line, re.I)
            if m:
                hosts += [h for h in m.group(1).split() if "*" not in h and "?" not in h]
    except OSError:
        pass
    return hosts


def register(sp):
    s = sp.add_parser("ssh-options", help="(used by the shell) ssh -oKey=Value candidates")
    s.add_argument("--current", default="")
    s.add_argument("--refresh", action="store_true")
    def run(a):
        if a.refresh:
            CACHE.unlink(missing_ok=True)
        print("\n".join(f"{v}\t{d}" for v, d in candidates(a.current)))
    s.set_defaults(fn=run)
