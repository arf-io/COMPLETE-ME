# COMPLETE-ME

**Tab completion for commands that never bothered to ship any.**

You press Tab on some tool nobody wrote completions for. Your shell offers you the contents of the
current directory, as if you might want to pass `node_modules` to `zerotier-cli`. COMPLETE-ME reads
the tool's help, works out what it takes, and completes it properly. It does this the first time
you press Tab, and it doesn't run anything it shouldn't to get there.

It works in fish, bash and zsh. It's one Rust binary: no daemon, no socket, no telemetry, and
nothing to babysit.

```
$ zerotier-cli <Tab>
info  join  leave  listnetworks  listpeers  orbit  peers  set  ...
$ zerotier-cli join <Tab>
8056c2e21c000001    (the network you joined last Tuesday)
```

---

## What it does

### Completions out of thin air
The first Tab on an unknown command generates its completions on the spot. Commands you run get
learned in the background, so the second Tab is instant too.

It reads help text in every dialect people have invented, and that's a lot of dialects:
- GNU, argparse, click, clap, cobra, commander.js, GLib, and man pages.
- Subcommand trees of any depth: `docker container ls --f<Tab>`.
- Commands with no flags at all, only subcommands (looking at you, `zerotier-cli`).
- git-style families split across programs: `clevis-luks-bind` on PATH becomes `clevis luks bind`.
- Allowed values: `--color=<Tab>` gives `always auto never`.
- Argument types: files where it says `FILE`, directories for `DIR`, and no files where it wants a number.
- Your own shell and Python scripts. It reads `case "$1" in`, `getopts`, usage heredocs and
  `argparse` straight from the source, and never executes them.

### It does not run your stuff
Running an arbitrary binary to find out what its flags are is a fine way to find out what it
does when it's run. So it works through these in order and stops at the first that answers:

1. Completions your shell already has always win and are never overwritten.
2. Man pages, which are just text.
3. The script's source code, read and not run.
4. `cmd --help`, and only inside a bubblewrap sandbox:
   - a read-only filesystem, with no network and no home to write to;
   - a private `/tmp` and `/run`, so no D-Bus, ssh-agent or docker socket;
   - and it gets killed after a few seconds.

It never runs a command bare, and never with `-h` (a surprising number of tools treat `-h` as
"halt"). Help text is treated as hostile input: names are validated, and descriptions are quoted
before your shell sees them.

### It remembers
Arguments you've used *in that exact position* come first. `zerotier-cli join <Tab>` offers the
network IDs you've actually joined, rather than asking you to retype sixteen hex digits from memory.

### It reads your clipboard, politely
Copied a git URL? `git clone <Tab>` offers it. A URL works for `curl`, `wget` and `yt-dlp`, and a
real path works for `cd`, `cp` and `less`. It's only offered when it's one line and actually a URL
or a path that exists, so the password you just copied stays where it is.

### Regex, for people who have to look it up every time
- Tab inside a `grep`, `rg`, `sed` or `find -regex` pattern lists the building blocks and what each
  one means. It also has ready-made recipes: IPv4, email, UUID, dates, semver, `TODO|FIXME`.
- It knows which dialect each tool speaks: `grep` is basic, `grep -E` extended, `rg` is Rust's,
  `-P` is PCRE, and `find` is, for reasons lost to history, Emacs.
- **alt-x** explains a pattern piece by piece and points out the mistakes. `\d` does nothing in
  `grep -E`. You knew that. Everyone forgets.
- **alt-r** is a live regex builder. You type a pattern and watch it match your files as you go.
  Press Enter and it lands on your command line, quoted correctly for your shell.

### `find`, without the dread
- `find . -exec rm <Tab>` offers `{}`, then `\;` or `+`, and says what each one does.
- `-type`, `-size`, `-mtime` and `-perm` suggest values. `-name` suggests the extensions actually
  sitting in your directory.
- **alt-x** reads the whole expression back in plain English and catches the classic traps:
  an action that only applies to one side of `-o`, or `-delete` placed before the test that was
  supposed to guard it.

### `ssh -oKey=Value`
Every `ssh_config` keyword, with what it does. `-oStrictHostKeyChecking=<Tab>` gives
`accept-new ask no off yes`. Ciphers, MACs and key exchanges come live from `ssh -Q`, so the list
matches your actual OpenSSH and not one from 2011.

### Bash typed into fish
Your fingers typed a bash loop into fish again. Press Enter anyway:

```
> for i in {1..3}; do echo "n=$i"; done
bash → fish
> for i in (seq 1 3)
      echo 'n='"$i"
  end
n=1
n=2
n=3
```

The translator is built in: a bash parser and a bash-to-fish translator, in Rust, with nothing
else to install. It began as a port of [babelfish](https://github.com/bouk/babelfish) (thanks,
Bouke; see `THIRD_PARTY.md`) and then kept going:

- Arithmetic (`$((a * b))`, `((i++))`, `let`, C-style `for`), `&>`, `|&`, `$!`, `$#`, `$0`, slices,
  indirection, case changes, `$'\t'`, `$(< file)`, array elements.
- `[[ $x == h* ]]` and `[[ $x =~ re ]]` match patterns for real.
- It fixes babelfish's mistakes:
  - `"$@"` stays separate words;
  - `x || { a; b; }` keeps its grouping;
  - here-docs don't grow an extra newline;
  - `eval "$(ssh-agent -s)"` translates what it evaluates.
- It's tested, not just promised. 106 snippets are compared with babelfish: 81 match byte for byte,
  and the other 25 are deliberate fixes, each with its reason written down. 95 of them are run in
  real bash and real fish, and both must print the same thing.
- It refuses what it can't translate faithfully (`select`, `coproc`, `declare -A`, extglobs,
  `set -e`, `read -t`) rather than guess.

**Safety check.** If the line deletes without asking, pipes a download into a shell, uses `sudo`,
writes to a disk or `/etc`, force-pushes, or `eval`s something, it stops and tells you why before
anything runs. Enter runs it; ctrl-c doesn't. Valid fish is never touched.
Turn it off with `set -U complete_me_no_bash 1`.

### Explain anything
alt-x on any command line describes each flag in it. `tar -xzvf` reads back as extract, gzip,
verbose, archive file, which is finally what it means.

### Aliases that keep their completions
```fish
complete-me alias gsw git switch                # gsw <Tab> completes branches
complete-me alias logs 'journalctl -u $1 -f'    # $1 goes where you put it; extra args follow
complete-me alias --abbr gst git status         # expands as you type
complete-me func mkcd 'mkdir -p $1; and cd $1'  # a real function
```

### Snippets
Type `,for`, `,if`, `,fn`, `,sw`, `,heredoc` and so on, then space. Each one expands in your
shell's own syntax, with the cursor where you need it. **alt-j** opens a picker, plus a
bash-to-fish phrasebook for the bits you never remember: `export X=1` is `set -gx X 1`, and
`<(cmd)` is `(cmd | psub)`.

---

## Shells

| | |
|---|---|
| **fish** | Everything. It was born here. |
| **bash** 4.2+ | Everything except bash-to-fish, for obvious reasons. It layers on top of bash-completion and doesn't replace it. Add `--with-blesh` for fish-style suggestions. |
| **zsh** | Everything except bash-to-fish. It sits on top of zsh's own completion and pulls in zsh-autosuggestions and zsh-syntax-highlighting if you have them. |
| **macOS** | Should work. There's no bubblewrap, so scripts are only read, and binaries are asked for `--help` minus a deny list. It hasn't been run on a real Mac yet. |

In bash and zsh, every piece can be switched off on its own: `COMPLETE_ME_NO_KEYS`, `_NO_ABBRS`,
`_NO_HISTORY`, `_NO_CLIPBOARD`, `_NO_PREGEN`, `_NO_PLUGINS`.

## Keys

| key | does |
|---|---|
| Tab | Completes, and generates completions first if a command has none. |
| alt-r | Live regex builder. |
| alt-x | Explains the current command line. |
| alt-j | Snippets, and the bash-to-fish phrasebook. |
| Enter | Runs the line. In fish, bash gets translated and checked first. |

---

## Install

```sh
git clone https://github.com/arf-io/COMPLETE-ME.git && cd COMPLETE-ME && ./install.sh
```

The installer finds your package manager (pacman, apt, dnf, zypper, apk, xbps or Homebrew) and
offers to install whatever's missing, falling back to Homebrew where there's no sudo. Then it
builds and links `~/.local/bin/complete-me`, adds one line to each shell you pick, runs the tests,
and gives you a short tour. Nothing gets installed without asking first.

Options: `--yes`, `--dry-run`, `--no-deps`, `--shells fish,bash`, `--with-blesh`, `--uninstall`.

If you'd rather type `make`: `make install`, `make install-yes`, `make test`, `make uninstall`.

### What you need

| | |
|---|---|
| Rust / cargo | Required. It builds the binary. |
| fish, bash 4.2+ or zsh | At least one. |
| bubblewrap | The sandbox for `--help`. Strongly recommended on Linux. |
| fzf | alt-r and alt-j. |
| wl-clipboard, xclip, xsel or pbpaste | Clipboard completion. |
| bash-completion | bash: the base it builds on. |
| zsh-autosuggestions, zsh-syntax-highlighting | zsh: grey suggestions and colours. |
| ble.sh *(optional)* | bash: the same, via `--with-blesh`. |

### How fast

One Tab, measured:

| Tab in… | old Python version | now |
|---|---|---|
| a regex pattern (`grep -E 'err<Tab>`) | 104 ms | 2.4 ms |
| `find -name <Tab>` | 67 ms | 2.3 ms |
| `ssh -oStrictHostKeyChecking=<Tab>` | 78 ms | 2.5 ms |
| a generated completion | 2 ms | 2 ms (it's plain fish either way) |

---

## Development

```sh
cargo test --release     # or: make test
```

- **Port check:** the Rust port is held to the Python original's golden files, byte for byte.
- **Parser:** tested against real `--help` output from real tools.
- **Hostile input:** malicious help text is tested to stay quoted.
- **Regex recipes:** every one is compiled by a real `grep`.
- **End to end:** real fish, bash (with and without bash-completion) and zsh (driven through a pty)
  are asked what they'd complete, in an isolated environment.

CI runs on our own Forgejo (`.forgejo/workflows/ci.yml`). Free as in beer, which was the
deciding factor.

## License

MIT; see `LICENSE`. Do what you like with it, just don't blame us when it completes `rm -rf` faster than you expected.

## Credits

The bash parser and translator started from babelfish (MIT) and mvdan/sh (BSD-3). Their notices
are in `THIRD_PARTY.md`.
