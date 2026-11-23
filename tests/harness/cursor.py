"""Drive an editor key binding and report the resulting buffer or exports."""
import errno
import os
import pty
import re
import select
import signal
import sys
import termios
import time

shell, runtime, left, right, *action = sys.argv[1:]
os.environ.update(CM_TEST_LEFT=left, CM_TEST_RIGHT=right, CM_TEST_RUNTIME=runtime, TERM="dumb" if shell == "fish" else "xterm")
setup = {
    "bash": r"""
        export PS1='cm-test> ' COMPLETE_ME_NO_PREGEN=1 COMPLETE_ME_NO_PLUGINS=1
        if [[ -n ${CM_TEST_GENERIC_COMPLETION-} ]]; then
            _minimal() { COMPREPLY=(); }
            _comp_complete_minimal() { _minimal; }
            __cm_vendor_loader() { complete -F "$CM_TEST_GENERIC_COMPLETION" -- "$1"; return 124; }
            complete -D -F __cm_vendor_loader
        fi
        source "$CM_TEST_RUNTIME"
        __cm_set() {
            READLINE_LINE=$CM_TEST_LEFT$CM_TEST_RIGHT
            READLINE_POINT=${#CM_TEST_LEFT}
        }
        __cm_get() { printf '\n@@BUFFER\t%s\n' "$READLINE_LINE"; }
        bind -x '"\C-x\C-a":__cm_set'
        bind -x '"\C-x\C-b":__cm_get'
        printf '\n@@READY\n'
    """,
    "fish": r"""
        source "$CM_TEST_RUNTIME"
        function __cm_set
            commandline -r -- "$CM_TEST_LEFT$CM_TEST_RIGHT"
            commandline -C (string length -- "$CM_TEST_LEFT")
        end
        function __cm_get
            printf '\n@@BUFFER\t%s\n' (commandline)
        end
        function __cm_observe_exec --on-event fish_preexec
            set -g __cm_submitted "$argv[1]"
        end
        function __cm_get_exports
            if string match -qr '^[[:blank:]]*set -gx CM_EXPORT ' -- "$__cm_submitted"
                printf '\n@@MODE\ttranslated\n'
            else
                printf '\n@@MODE\tnative\n'
            end
            /bin/sh -c 'printf "\n@@EXPORTS\t%s|%s|%s\n" "${CM_EXPORT-unset}" "${CM_EXPORT_EMPTY-unset}" "${PATH%%:*}"'
            if contains -- "set -gx CM_EXPORT 'two words'" (history search --exact -- "set -gx CM_EXPORT 'two words'" 2>/dev/null)
                printf '@@HISTORY\tfound\n'
            else
                printf '@@HISTORY\tabsent\n'
            end
        end
        if set -q CM_TEST_CUSTOM_EXPORT
            function export
                set -gx CM_EXPORT custom
            end
        end
        bind \cx\ca __cm_set
        bind \cx\cb __cm_get
        bind \cx\ce __cm_get_exports
        bind \cx\cd complete
        printf '\n@@READY\n'
    """,
    "zsh": r"""
        PS1='cm-test> ' COMPLETE_ME_NO_PREGEN=1 COMPLETE_ME_NO_PLUGINS=1
        source "$CM_TEST_RUNTIME"
        __cm_set() {
            BUFFER=$CM_TEST_LEFT$CM_TEST_RIGHT
            CURSOR=${#CM_TEST_LEFT}
        }
        __cm_get() { printf '\n@@BUFFER\t%s\n' "$BUFFER"; }
        zle -N __cm_set
        zle -N __cm_get
        bindkey '^X^A' __cm_set
        bindkey '^X^B' __cm_get
        printf '\n@@READY\n'
    """,
}
pid, tty = pty.fork()
if pid == 0:
    flags = {"bash": ["--noprofile", "--norc"], "fish": ["--no-config"], "zsh": ["-f"]}[shell]
    os.execvp(shell, [shell, *flags, "-i"])

transcript = b""


def read_until(pattern):
    global transcript
    end = time.monotonic() + 10
    while time.monotonic() < end:
        if match := re.search(pattern, transcript):
            return match
        readable, _, _ = select.select([tty], [], [], max(0, end - time.monotonic()))
        if not readable:
            break
        try:
            data = os.read(tty, 8192)
        except OSError as e:
            if e.errno != errno.EIO:
                raise
            break
        transcript += data
    raise RuntimeError(f"PTY did not emit {pattern!r}: {transcript!r}")


def wait_for_editor():
    # The setup command prints READY before the reader switches back to raw
    # input. In cooked mode the terminal can translate a queued CR into LF.
    end = time.monotonic() + 10
    while time.monotonic() < end:
        if not termios.tcgetattr(tty)[3] & termios.ICANON:
            return
        time.sleep(0.005)
    raise RuntimeError("PTY did not return to editor input mode")


try:
    # Send setup as one source command so each line is parsed by the shell outside the editor.
    import tempfile
    with tempfile.NamedTemporaryFile(mode="w") as config:
        config.write(setup[shell])
        config.flush()
        os.write(tty, f"source {config.name}\n".encode())
        read_until(rb"\r?\n@@READY\r?\n")
        if action in (["enter"], ["confirm"], ["decline"]):
            wait_for_editor()
            os.write(tty, b"\x18\x01\r")
            if action != ["enter"]:
                read_until(rb"Apply to this fish session\? \[y/N\] ")
                os.write(tty, b"y\r" if action == ["confirm"] else b"n\r")
            os.write(tty, b"\x18\x05")
            result = read_until(rb"\r?\n@@MODE\t([^\r\n]*)\r?\n\r?\n@@EXPORTS\t([^\r\n]*)\r?\n@@HISTORY\t([^\r\n]*)\r?\n")
            print(result[1].decode())
            print(result[2].decode())
            print(result[3].decode())
        else:
            if action == ["new-command"]:
                os.link("script.fixture", "bin/ship-demo")
            key = {"recipe": b"\x1bR", "native": b"\x18\x04", "list-tab": b"\t\t"}.get(next(iter(action), ""), b"\t")
            os.write(tty, b"\x18\x01" + key + b"\x18\x02")
            result = read_until(rb"\r?\n@@BUFFER\t([^\r\n]*)\r?\n")
            print(transcript.decode(errors="replace") if action == ["list-tab"] else result[1].decode())
            if action == ["calculator"]:
                transcript = transcript[result.end():]
                os.write(tty, b"\x01\x0bans/4=\t\x18\x02")
                result = read_until(rb"\r?\n@@BUFFER\t([^\r\n]*)\r?\n")
                print(result[1].decode())
finally:
    os.close(tty)
    try:
        os.kill(pid, signal.SIGTERM)
    except ProcessLookupError:
        pass
    os.waitpid(pid, 0)
