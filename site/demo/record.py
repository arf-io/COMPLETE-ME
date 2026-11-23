"""Capture keystrokes in clean shell sessions and write the static browser recording."""

import codecs
import errno
import fcntl
import json
import os
from pathlib import Path
import pty
import select
import shlex
import signal
import struct
import subprocess
import termios
import tempfile
import time

import pyte

ROOT = Path(__file__).resolve().parents[2]
SHELLS = ("bash", "fish", "zsh")
PROMPT = "demo> "
SCENES = (
    ("windows", "tmux windows", "tmux a -t work:", ["Tab", "Tab"],
     "A colon asks for windows inside the work session. Bash lists candidates on the second Tab.",),
    ("session", "tmux sessions", "tmux a -t wor", ["Tab"],
     "Complete the session name before attaching.",),
    ("python", "Python options", "ship-demo --fo", ["Tab"],
     "The sample Python script declares its options with argparse.",),
    ("paths", "Directories", "cd pro", ["Tab"],
     "A directory on disk is enough for the shell’s own filename completion.",),
    ("calculator", "Calculator", "2*34=", ["Tab"],
     "COMPLETE-ME fills the answer into the command line.",),
    ("snippets", "Snippets", "", ["Alt+J"],
     "Alt+J opens the COMPLETE-ME snippet picker.",),
)
KEYS = {"Tab": b"\t", "Alt+J": b"\x1bj"}
BASH_COMPLETION = next((path for path in map(Path, (
    "/opt/homebrew/share/bash-completion/bash_completion",
    "/usr/share/bash-completion/bash_completion",
    "/usr/local/share/bash-completion/bash_completion",
)) if path.is_file()), None)


def config(shell, enabled):
    runtime = shlex.quote(str(ROOT / "shell" / shell / f"complete_me.{shell}"))
    if shell == "fish":
        text = "set -g fish_greeting\nset -g fish_autosuggestion_enabled 0\nfunction fish_prompt; printf 'demo> '; end\n"
    else:
        text = f"PS1={shlex.quote(PROMPT)}\n"
        if shell == "bash" and BASH_COMPLETION:
            text += f"source {shlex.quote(str(BASH_COMPLETION))}\n"
        if shell == "zsh":
            text += "autoload -Uz compinit\ncompinit -i\n"
    return text + (f"source {runtime}\n" if enabled else "")


def capture(shell, enabled, home, env):
    rc = home / (".zshrc" if shell == "zsh" else "demo.rc")
    rc.write_text(config(shell, enabled))
    commands = {
        "bash": ["bash", "--noprofile", "--rcfile", str(rc), "-i"],
        "fish": ["fish", "--no-config", "--init-command", f"source {shlex.quote(str(rc))}", "-i"],
        "zsh": ["zsh", "-d", "-i"],
    }
    rows, cols = 15, 57
    pid, tty = pty.fork()
    if pid == 0:
        fcntl.ioctl(0, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))
        os.chdir(home)
        os.execvpe(shell, commands[shell], env)

    screen = pyte.Screen(cols, rows)
    # A few shell editors ask the terminal for its position during startup.
    screen.write_process_input = lambda text: os.write(tty, text.encode())
    stream = pyte.Stream(screen)
    decode = codecs.getincrementaldecoder("utf-8")(errors="replace")

    def read():
        try:
            chunk = os.read(tty, 8192)
        except OSError as error:
            if error.errno == errno.EIO:
                raise RuntimeError(f"{shell} exited before the recording ended") from error
            raise
        if not chunk:
            raise RuntimeError(f"{shell} closed its terminal")
        stream.feed(decode.decode(chunk))

    def drain(minimum):
        mark = time.monotonic() + minimum
        deadline = mark + 4
        while time.monotonic() < deadline:
            if select.select([tty], [], [], 0.12)[0]:
                read()
            elif time.monotonic() >= mark:
                return

    def snapshot():
        return "\n".join(line.rstrip() for line in screen.display).strip("\n")

    try:
        deadline = time.monotonic() + 12
        while PROMPT not in "\n".join(screen.display):
            remaining = deadline - time.monotonic()
            if remaining <= 0 or not select.select([tty], [], [], remaining)[0]:
                raise RuntimeError(f"{shell} did not reach its prompt: {snapshot()!r}")
            read()
        captures = {}
        for key, _, command, strokes, _ in SCENES:
            os.write(tty, b"\x03")
            drain(0.15)
            os.write(tty, b"\x0c")
            drain(0.15)
            os.write(tty, command.encode())
            drain(0.15)
            frames = [snapshot()]
            for stroke in strokes:
                os.write(tty, KEYS[stroke])
                drain(0.8)
                frames.append(snapshot())
            if PROMPT.rstrip() not in frames[0] or command not in frames[0]:
                raise RuntimeError(f"No command line in {shell} {key}: {frames[0]!r}")
            captures[key] = frames
        return captures
    finally:
        os.close(tty)
        try:
            os.kill(pid, signal.SIGTERM)
        except ProcessLookupError:
            pass
        os.waitpid(pid, 0)


def main():
    versions = {}
    for tool in (*SHELLS, "tmux"):
        flag = "-V" if tool == "tmux" else "--version"
        versions[tool] = subprocess.check_output([tool, flag], text=True).splitlines()[0]
    versions["bashCompletion"] = bool(BASH_COMPLETION)
    recordings = [{
        "id": key, "title": title, "command": command, "keys": strokes, "hint": hint, "shells": {},
    } for key, title, command, strokes, hint in SCENES]

    with tempfile.TemporaryDirectory(prefix="cm-record-", dir="/tmp") as scratch:
        root = Path(scratch)
        env = {
            "PATH": os.environ["PATH"], "TERM": "xterm-256color", "LANG": "en_US.UTF-8",
            "TMUX_TMPDIR": scratch, "HOME": scratch, "COMPLETE_ME_NO_REMOTE": "1",
            "COMPLETE_ME_NO_CLIPBOARD": "1", "COMPLETE_ME_NO_PREGEN": "1", "COMPLETE_ME_NO_PLUGINS": "1",
        }
        def tmux(*args):
            subprocess.run(["tmux", "-f", "/dev/null", *args], env=env, check=True)
        tmux("new-session", "-d", "-s", "work", "-n", "editor", "sleep 180")
        try:
            for name in ("build", "logs"):
                tmux("new-window", "-d", "-t", "work", "-n", name, "sleep 180")
            for shell in SHELLS:
                print(f"Recording {shell}", flush=True)
                for enabled in (False, True):
                    home = root / f"{shell}-{enabled}"
                    (home / "bin").mkdir(parents=True)
                    (home / "projects").mkdir()
                    script = home / "bin" / "ship-demo"
                    script.write_text(
                        "#!/usr/bin/env python3\nimport argparse\nparser = argparse.ArgumentParser()\n"
                        "parser.add_argument('--format', choices=['json', 'table', 'text'])\n"
                        "parser.parse_args()\n"
                    )
                    script.chmod(0o700)
                    session_env = dict(env, HOME=str(home), ZDOTDIR=str(home),
                                       PATH=f"{home / 'bin'}:{env['PATH']}")
                    captures = capture(shell, enabled, home, session_env)
                    for recording in recordings:
                        lanes = recording["shells"].setdefault(shell, {})
                        lanes["enabled" if enabled else "stock"] = captures[recording["id"]]
        finally:
            tmux("kill-server")
    data = {"versions": versions, "scenes": recordings}
    output = "/* Generated by site/demo/record.py. */\nwindow.completeMeRecordings = "
    (ROOT / "site/dist/recordings.js").write_text(output + json.dumps(data, ensure_ascii=False, indent=2) + ";\n")


if __name__ == "__main__":
    main()
