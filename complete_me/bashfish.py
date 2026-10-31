"""Bash typed into fish: translate it with babelfish, check it for danger, hand it back.

Used by the fish Enter binding: only lines that are NOT valid fish but ARE valid bash get here.
Output protocol (stdout):
    RISK<TAB>reason          zero or more lines; any RISK means the shell asks before running
    ---FISH---
    <translated fish code>
"""
from __future__ import annotations

import re
import shutil
import subprocess
import sys

# (pattern, reason). Checked against the bash source.
RISKS = [
    (r"\brm\s+(-[a-zA-Z]*[rRf][a-zA-Z]*\s+)+", "rm -r/-f: deletes without asking"),
    (r"\brm\b[^|;&\n]*\s(/|~|\$HOME|/\*|\*)(\s|$|;|&)", "rm on / , ~ or a bare *"),
    (r"\b(curl|wget|fetch)\b[^|\n]*\|\s*(sudo\s+)?(ba|z|da|k)?sh\b", "downloads a script and runs it"),
    (r"\b(sudo|doas|pkexec|su)\b", "runs as root"),
    (r"\bdd\b[^\n]*\bof=", "dd writes raw data to a file or disk"),
    (r"\b(mkfs(\.\w+)?|wipefs|fdisk|sfdisk|parted|gdisk|shred|blkdiscard)\b", "formats, wipes or repartitions disks"),
    (r">\s*/dev/(sd|nvme|hd|vd|mmcblk|disk)", "writes straight to a disk device"),
    (r":\(\)\s*\{\s*:\s*\|\s*:\s*&\s*\}\s*;?\s*:", "fork bomb"),
    (r"\bchmod\s+(-[a-zA-Z]*R[a-zA-Z]*\s+)?[0-7]*7[0-7]{2}\s+/", "makes system paths world-writable"),
    (r"\bchown\s+-[a-zA-Z]*R\b[^\n]*\s/(\s|$)", "recursively re-owns /"),
    (r"(^|[^>])>\s*~?/?[\w./-]*\.(bashrc|zshrc|profile|bash_profile)\b", "overwrites a shell startup file"),
    (r">\s*/etc/", "overwrites a file in /etc"),
    (r"\b(shutdown|reboot|poweroff|halt)\b", "shuts down or reboots"),
    (r"\bkill\s+-9\s+-1\b|\bkillall\b|\bpkill\b", "kills processes broadly"),
    (r"\bgit\s+push\b[^\n]*(--force|-f\b)", "force-pushes"),
    (r"\bgit\s+(reset\s+--hard|clean\s+-[a-zA-Z]*f)", "discards uncommitted work"),
    (r"\beval\b", "eval runs text as code"),
    (r"\bhistory\s+-c\b", "clears your history"),
]

RANGE = re.compile(r"(?<![\w$'\"]){(-?\d+)\.\.(-?\d+)(?:\.\.(-?\d+))?}")


def risks(src: str) -> list[str]:
    found = []
    for pat, why in RISKS:
        if re.search(pat, src) and why not in found:
            found.append(why)
    return found


def preprocess(src: str) -> str:
    """Things babelfish leaves as bash-isms: {1..10} brace ranges -> $(seq 1 10)."""
    def seq(m):
        a, b, step = m.group(1), m.group(2), m.group(3)
        if step:
            return f"$(seq {a} {step} {b})"
        return f"$(seq {a} {'-1 ' if int(b) < int(a) else ''}{b})"
    return RANGE.sub(seq, src)


def translate(src: str) -> tuple[str | None, str]:
    exe = shutil.which("babelfish")
    if not exe:
        return None, "babelfish is not installed (brew install babelfish, or: go install github.com/bouk/babelfish@latest)"
    try:
        p = subprocess.run([exe], input=preprocess(src), capture_output=True, text=True, timeout=5)
    except (OSError, subprocess.TimeoutExpired) as e:
        return None, f"babelfish failed: {e}"
    if p.returncode != 0 or not p.stdout.strip():
        return None, (p.stderr.strip() or "babelfish could not translate this").splitlines()[0]
    return p.stdout.rstrip("\n"), ""


def is_bash(src: str) -> bool:
    if not shutil.which("bash"):
        return False
    try:
        return subprocess.run(["bash", "-n"], input=src, capture_output=True, text=True, timeout=3).returncode == 0
    except (OSError, subprocess.TimeoutExpired):
        return False


def run(a) -> int:
    src = a.source if a.source is not None else sys.stdin.read()
    if not is_bash(src):
        print("not valid bash either", file=sys.stderr)
        return 2
    fish, err = translate(src)
    for r in risks(src):
        print(f"RISK\t{r}")
    if fish is None:
        print(err, file=sys.stderr)
        return 1
    print("---FISH---")
    print(fish)
    return 0


def register(sp):
    p = sp.add_parser("bash2fish", help="translate bash to fish (babelfish) with a safety check")
    p.add_argument("source", nargs="?", help="bash code (default: stdin)")
    p.set_defaults(fn=run)
