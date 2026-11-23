# COMPLETE-ME

[![Test](https://github.com/arf-io/COMPLETE-ME/actions/workflows/test.yml/badge.svg?branch=main)](https://github.com/arf-io/COMPLETE-ME/actions/workflows/test.yml) [![Provider verification](https://github.com/arf-io/COMPLETE-ME/actions/workflows/verification.yml/badge.svg?branch=main)](https://github.com/arf-io/COMPLETE-ME/actions/workflows/verification.yml)

**Your shell can finish that.** COMPLETE-ME adds Tab completion for flags, subcommands, and live
values in Bash, zsh, and fish. One Rust binary generates missing command completions and draws
live values from the tools you already use.

[Install](#install) · [Try it in a terminal](#try-it-in-a-terminal) · [Keys](#keys) ·
[Completion reference](docs/completion.md) · [Verification](docs/verification.md)

The browser demo compares recorded Bash, fish, and zsh output for the same keystrokes, with
shell defaults and COMPLETE-ME enabled. [Preview the demo locally](site/README.md) or read the
[press announcement](docs/press-release.md).

## Try it in a terminal

Press Tab where you see the marker.

| Type | Pick |
|---|---|
| `tmux a -t work:<Tab>` | A window in the `work` session |
| `docker compose logs <Tab>` | A service from the selected Compose project |
| `ssh -oIdentityFile=~/.ssh/<Tab> dev@example.com` | An identity-file path while preserving the destination |
| `git config set --global user.email <Tab>` | Your current Git email value, ready to edit |
| `proxmox lab --node pve02 qmrestore vault:<Tab>` | A VM archive on a configured cluster node |
| `aws s3 cp s3://example-bucket/logs/<Tab> ./download` | An object key or prefix in the chosen AWS scope |

Use `complete-me doctor` when Tab behaves differently than you expect. Press **Alt+J** to open
the snippet picker.

## Calculate on Tab

Type `2*34=` and press Tab to insert `68`. On the same line, type a space and an opening
parenthesis, then press Tab to insert the previous result as an operand. Finish with
`/3)^2=` and press Tab again. A new line can use `ans` to refer to the previous answer.

For scripts or a direct calculation, run:

    complete-me calc 'sqrt(144) + log(1000, 10)'
    complete-me calc 'hypot(3,4) + 5!'

The calculator supports arithmetic, powers, scientific notation, trig, logs, rounding, and
factorials with double-precision numbers. [Calculator syntax and examples](docs/completion.md#calculator)

## What it does

### Completions out of thin air
The first Tab on an unknown command generates its completions on the spot. Commands you run get
learned in the background, so the second Tab is instant too.

It reads help text in several formats:
- GNU, argparse, click, clap, cobra, commander.js, GLib, and man pages.
- Nested subcommands, declared aliases, and path-specific options. A shared parser resolves them in fish, Bash, and zsh.
- Commands with no flags at all, only subcommands (looking at you, `zerotier-cli`).
- git-style families split across programs: `clevis-luks-bind` on PATH becomes `clevis luks bind`.
- Allowed values: `--color=<Tab>` gives `always auto never`.
- Argument types: files where it says `FILE`, directories for `DIR`, and no files where it wants a number.
- Your own shell and Python scripts. It reads `case "$1" in`, `getopts`, usage heredocs and
  `argparse` straight from the source, and never executes them.

### Where generated completions come from

COMPLETE-ME keeps installed shell completions. When it needs to generate one, it reads manual
pages and script source and can use a bounded `cmd --help` probe. With bubblewrap installed, the
probe runs in a read-only filesystem with private `/tmp` and `/run` directories and an isolated
network. Without bubblewrap, probing follows a deny list and reads scripts as source.

Help discovery only uses `--help` when invoking a command. Names are validated and descriptions
are quoted before your shell sees them.

### Live values

| Command | Live candidates |
|---|---|
| `tmux` | Subcommands and aliases, followed by sessions, windows, panes or clients for the switch that accepts them. `tmux a -t <Tab>` lists sessions. `tmux a -t work:<Tab>` and `tmux select-window -t <Tab>` list windows. `-L` and `-S` select the server. |
| `docker`, `podman` | Container names and IDs with image and status, and image references and IDs with size and age. Common network and volume arguments also complete. |
| `docker compose`, `docker-compose` | Project services and profile names, using the selected Compose files. |
| `zerotier-cli` | Network IDs joined by the current node, with network name, status, type and addresses for `join`, `leave`, `get` and `set`. History can supply IDs from earlier commands. |
| `git` | Config subcommands, scope flags, keys and editable current values for common preferences. Local and remote-tracking refs show commit details, and `fetch` and `push` complete configured remotes. |
| `gh` | Pull request and issue numbers with titles and state in the selected repository. |
| `just`, `task` | Just recipes and public tasks or aliases from a local Taskfile. |
| `kubectl` | Contexts, namespaces and names of common resources such as pods and deployments. Resource completion queries the selected context and namespace. |
| `kube` | Explicit Kubernetes context and namespace selection. `@all` offers `namespace/name` for supported pod and resource commands. |
| `aws`, `vultr-cli`, `hf`, `cm-cloud` | AWS profiles, regions, resource IDs, and S3 objects and prefixes. Also covers Vultr inventory, local Hugging Face repositories, Lambda Cloud instances and DreamHost DNS names. |
| AI agent CLIs | Commands and flags, a configurable local model and session catalog, and local session IDs for Codex and Claude Code. |
| `systemctl`, `journalctl` | System or user units with state, including `journalctl --user-unit=<Tab>`. |
| `npm`, `pnpm`, `yarn`, `bun` | Scripts from the current project's `package.json` after `run`. |
| `brew` | Installed formulae with versions and installed cask names for commands such as `upgrade`, `reinstall` and `uninstall`. |
| `pct`, `qm`, `pvesm`, `vzdump`, `qmrestore` | Proxmox guests, snapshots, migration nodes, storage, backup archives, and restore IDs. |
| `zfs`, `zpool` | Datasets, volumes, snapshots, and pools. `complete-me zfs-path` completes paths within a mounted local dataset. |
| `dd` | Linux device paths, partition starts, and logical and physical sector sizes for the input or output. |
| `tcpdump`, `tshark`, `wireshark`, `ip`, `route` | Local interfaces and command-specific routing, capture, and filter arguments. |
| `nmap` | Help, version, interface-listing, and report output flags. |

Live queries use your current permissions and selected runtime. GitHub results are cached for 20
seconds per context. Fish and zsh display candidate descriptions in their menus. Bash's Readline
menu displays values. Use `complete-me live` to see values and descriptions together.
Run `complete-me live --coverage` to see live argument families and lookup sources by provider.

For example, press Tab after `git config set --global user.email ` to put the current global
email on the line for editing. The same value completion works with the older spelling
`git config --global user.email `. Git writes the change when you submit the final command.

```sh
complete-me live --current web -- docker compose logs
complete-me explain --completion --line 'gh pr view -R arf-io/COMPLETE-ME '
```

The completion report identifies the source, selected scope, duration, and candidate count. Run
`complete-me doctor --line 'docker compose logs '` in your configured shell to also see the Tab
binding, active shell handler, and generated cache status. See the
[completion reference](docs/completion.md) for runtime selectors and offline mode.

### Proxmox over SSH

On a Proxmox node, `pct enter <Tab>` lists running containers and `qm status <Tab>` lists VMs.
From your workstation, put your existing SSH aliases, one per line, in
`${XDG_CONFIG_HOME:-$HOME/.config}/complete-me/proxmox-hosts`. Then type
`proxmox lab-pve pct enter <Tab>`. The menu can show `100` next to `dns-prod · running · lab-pve`.
Selecting it inserts `100`. Press Enter to open the container shell with an SSH PTY.

Export `COMPLETE_ME_PROXMOX_IP=1` to add available IP addresses when a prefix narrows the list
to one guest. See [setup and examples](docs/completion.md#proxmox-and-zfs).

For a cluster, map each Proxmox node to a trusted SSH alias in `complete-me/proxmox-clusters`.
`proxmox lab pct enter` can then show containers across those nodes and route the submitted
command to the current owner. Kubernetes uses a similar workflow with
`kube prod @all logs`, which completes a qualified pod name such as `build/worker`.

Use `proxmox lab --node pve02 vzdump <Tab>` to select a guest on that node, or
`proxmox lab --node pve02 qmrestore vault:<Tab>` to browse VM backups in that storage.
For containers, use `pct restore`. See [backup and restore examples](docs/completion.md#backup-and-restore).

### Cloud and agent CLIs

`aws --profile work --region us-east-2 ec2 start-instances --instance-ids` completes instance
IDs with names and state. `cm-cloud lambda instances` and `cm-cloud dreamhost records` offer
read-only inventories. `hf download` can reuse repositories from the local Hugging Face cache.

The agent adapters cover Codex, Claude Code, Gemini CLI, Antigravity, Hermes, Cline, OpenCode,
Aider, and more. Model names and session references can be added in `complete-me/agents.json`.
Tab reads local metadata and never launches an agent. See the [provider reference](docs/completion.md#cloud-and-agent-providers)
and the [verification package](docs/verification.md).

### Disk and byte inspection

On Linux, try Tab at `dd if=/dev/`, `dd if=/dev/sda skip=`, or `dd of=/dev/sdb obs=`.
The device menu shows partition starts and logical and physical sector sizes. `skip=` and `seek=`
complete the byte offset of a partition on the selected disk. A partition opened directly starts
at zero relative to that device. Byte suffixes require GNU `dd` 9.1 or later.

Run `cm-bytes FILE` to find file headers, known PNG chunks, and long printable regions with byte
offsets. With the cursor at `hexdump -s <Tab> FILE`, those offsets are live candidates. See the
[disk, byte, and network reference](docs/completion.md#disk-byte-and-network-helpers).

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
Keywords from the installed `ssh_config` manual come with descriptions. `-oStrictHostKeyChecking=<Tab>`
gives `accept-new ask no off yes`. Algorithm lists come from `ssh -Q`. Path options such as
`-oIdentityFile=<Tab>` start in `~/.ssh/`. Use an explicit directory to complete from it.

Tab uses the token at the cursor. It can extend either token below or fill in a missing part inside
an existing filename or keyword while keeping the following text in place.

```text
ssh -oPub<Tab> -oIdentityFile=~/.ssh/<Tab> user@ssh.com
```

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

Standalone Bash-style assignments also translate on Enter with the fish integration loaded.
For example, `export EDITOR=nvim` becomes `set -gx EDITOR 'nvim'`. This works with quoted values,
empty values, independent assignments on one line, and expressions such as
`export PATH="$HOME/bin:$PATH"`. These changes apply to the current fish session and its child
processes. Fish handles export listings, custom `export` functions, and valid fish commands that
mix exports with other operations as usual. A leading space keeps the rewritten line out of fish
history.

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
anything runs. Enter runs it; ctrl-c doesn't.
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

In fish, `complete-me alias --list` shows managed aliases and fish aliases from the running shell.
Use `complete-me func --list` to see fish functions and `complete-me func --save NAME` to save a
function already defined in the shell. A function created with `complete-me func NAME 'BODY'`
is loaded immediately. `complete-me path list` prints `PATH` in search order. Add directories with
`complete-me path add DIRECTORY…`, which uses `fish_add_path` and accepts its options.

Run `complete-me path why python` from fish, Bash, or zsh to see what the shell selects, which
executables it shadows, and which directories occur more than once in `PATH`.

### Preview environment changes

```sh
complete-me env diff .env.local
```

The preview shows which exported variables would be added, changed, or removed. Values stay
redacted. In fish, `complete-me env apply .env.local` shows the same preview and asks before
applying it to the current session. `complete-me env undo` restores the previous values and fish
variable attributes from the last successful apply. Use `-` to read pasted assignments from stdin.
[Environment file syntax](docs/completion.md#environment-changes)

### Save commands with live arguments

Put this in `.complete-me/recipes.json` in your project:

```json
{
  "logs": ["docker", "compose", "logs", "-f", "{service}"]
}
```

Type `complete-me recipe expand logs ` and press Tab to choose a service. Then press
**Alt+Shift+R** to replace the recipe invocation with the expanded command in the editor. Press
Enter when you're ready to run it. Works in fish, Bash, and zsh.
[Recipe format and commands](docs/completion.md#project-recipes)

### Snippets
Type `,for`, `,if`, `,fn`, `,sw`, `,heredoc` and so on, then space. Each one expands in your
shell's own syntax, with the cursor where you need it. **alt-j** opens a picker, plus a
bash-to-fish phrasebook for the bits you never remember: `export X=1` is `set -gx X 1`, and
`<(cmd)` is `(cmd | psub)`.

---

## Shells

| | |
|---|---|
| **fish 4+** | Generated and live completions, candidate descriptions, snippets, Bash-to-fish translation, and function and PATH management. |
| **Bash 4.2+** | Generated and live completions, snippets, and the regex and explanation tools. Works with or without Bash completion. `--with-blesh` adds optional suggestions and highlighting. |
| **zsh** | Generated and live completions, candidate descriptions, snippets, and the regex and explanation tools. Uses installed zsh-autosuggestions and zsh-syntax-highlighting when available. |
| **macOS** | Tested with fish, Bash and zsh. Help discovery reads scripts and can call `--help` on binaries that pass its deny list when bubblewrap is unavailable. |

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
builds and installs `~/.local/bin/complete-me`, connects the shells you pick, runs a smoke
check, and gives you a short tour. Run `make test` for the full suite. Use `--binary PATH` to
install a trusted prebuilt executable with the matching checkout, for example on a host without Rust.

Options: `--yes`, `--dry-run`, `--no-deps`, `--binary PATH`, `--shells fish,bash`, `--with-blesh`, `--uninstall`.

If you'd rather type `make`: `make install`, `make install-yes`, `make test`, `make uninstall`.

### What you need

| | |
|---|---|
| Rust / cargo | Required. It builds the binary. |
| fish 4+, Bash 4.2+, or zsh | At least one. |
| bubblewrap | The sandbox for `--help`. Strongly recommended on Linux. |
| fzf | alt-r and alt-j. |
| wl-clipboard, xclip, xsel or pbpaste | Clipboard completion. |
| bash-completion | bash: the base it builds on. |
| zsh-autosuggestions, zsh-syntax-highlighting | zsh: grey suggestions and colours. |
| ble.sh *(optional)* | bash: the same, via `--with-blesh`. |

## Development

```sh
make check
COMPLETE_ME_REQUIRE_TEST_TOOLS=1 cargo test --locked --release
cargo clippy --locked --release --all-targets -- -D warnings
```

Tests cover help parsing, snapshot output, generated completions, live data providers, and actual
Tab behavior in fish, Bash, and zsh. The suite uses a temporary Git repository and an isolated
tmux server for stateful tests. Provider fixtures cover services that require their own runtime
or login. The GitHub workflow runs on Linux and macOS. A separate hosted workflow exercises a
disposable Docker Compose project and the Linux help sandbox. See [testing](docs/testing.md) for
the full matrix, opt-in commands, and coverage measurement.

## License

MIT; see `LICENSE`. Do what you like with it, just don't blame us when it completes `rm -rf` faster than you expected.

## Credits

The bash parser and translator started from babelfish (MIT) and mvdan/sh (BSD-3). Their notices
are in `THIRD_PARTY.md`.
