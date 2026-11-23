# Completion reference

COMPLETE-ME integrates with fish 4+, Bash 4.2+, and zsh on Linux and macOS. It can generate
completions from help, manuals, and script source. Live providers handle arguments whose values
depend on a running service, local project, configured account, or repository.

## Work inside a command line

Place the cursor at the end of a token or inside one. Tab uses the context to its left. Inside a
token, it narrows candidates to those that fit the existing suffix and preserves later arguments,
quotes, and pipeline text. For example, typing Tab at `la<Tab>.key` can fill in `laboratory.key`.
Fish lists multiple matching edits or offers them through `fzf` when available.

```text
ssh -oPub<Tab> -oIdentityFile=~/.ssh/<Tab> user@ssh.com
```

For generated completions, fish, Bash, and zsh use the same parser for nested subcommands,
declared aliases, separate and attached option values, short-option clusters, and the end-of-options
marker `--`. A flag declared on the active subcommand takes precedence over a flag with the
same spelling on a parent. The parser uses the options and aliases discovered for that command.

Generated files in an existing cache can be refreshed with `complete-me gen --force COMMAND`.
Use `complete-me show --json COMMAND` to inspect the model. Bash uses native Readline candidates.
Its standard completion menu lists candidate values. Fish and zsh can also show descriptions.

## Live providers

| Command | What Tab offers | Scope and availability |
|---|---|---|
| `tmux a -t ` | Sessions and their window counts | Uses the current server or the socket specified by `-L` or `-S`. Live discovery uses tmux's `-N` switch. |
| `tmux a -t work:`, `tmux select-window -t ` | Window targets and names | Other switches can select panes or clients. |
| `docker exec -it `, `podman logs ` | Container names and IDs with image and status | Uses the current daemon or configured connection. `exec` selects from running containers. |
| `docker run `, `podman image rm ` | Image references and IDs with size and age | Common network and volume arguments have their own candidates. |
| `docker --context=`, `podman --connection ` | Context or connection names, marked when current | Reads local CLI configuration, including in offline mode. |
| `docker service logs `, `docker plugin inspect `, `podman pod stop ` | Configured management targets and status | Uses the selected runtime. Docker also completes Swarm configs, secrets and stacks. Podman completes secret names. |
| `docker compose logs `, `docker-compose up ` | Services declared by the selected Compose project | Uses its file, project directory, profile, and environment-file selections. The Compose CLI must support the provider's `config` flags. |
| `docker compose --profile=` | Profile names | Uses the selected Compose config. |
| `zerotier-cli leave ` | Network IDs with name, state, type, and addresses | Lists networks joined by the current node, using the shell's existing permissions. |
| `git switch `, `git fetch ` | Refs or configured remotes, depending on the argument | Uses the current repository or the one selected by `git -C`. |
| `git log trunk..`, `git restore --source=` | Refs inside revision ranges or options | Uses local heads, tags and remote-tracking refs. |
| `git show HEAD:`, `git add `, `git restore -- ` | Revision paths, changed paths and tracked paths | Reads the tree or index. Git transport access is disabled during lookup. |
| `git worktree remove `, `git stash pop `, `git submodule update -- ` | Worktrees, stash entries and submodule paths | Reads local repository metadata. |
| `git config se`, `git config set --glo`, `git config get user.em` | Subcommands, scope flags and keys | Uses the modern subcommands with Git 2.46+, and understands legacy flag syntax. |
| `git config set --global user.email ` | Editable current value | Supports common scalar settings in the selected local, global, worktree, system or explicit file scope. |
| `gh pr view `, `gh issue close ` | Recent pull request or issue numbers with title, state, and update time | Uses the current GitHub CLI login and repository, including `-R` and `GH_REPO`. The listing is capped at 50 results. |
| `kubectl --context=`, `kubectl get pods ` | Contexts, namespaces, and common resources | Queries the configured cluster and selected namespace when required. |
| `systemctl status `, `journalctl --user-unit=` | System or user units and state | Uses the selected system or user manager. |
| `npm run `, `pnpm run `, `yarn run `, `bun run ` | Package scripts | Reads the project manifest. Workspace selection is handled by the shell's ordinary completion. |
| `just ` | Recipe names and comments | Uses Just's recipe listing, including an explicitly selected Justfile. |
| `task ` | Public tasks and aliases | Reads the selected local Taskfile. Names from included Taskfiles and computed task definitions are outside this provider's coverage. |
| `brew upgrade ` | Installed formulae and casks | Formulae show installed versions. |
| `pct enter `, `qm status ` | Numeric guest IDs, with name and state | Reads inventory on the local Proxmox node. The console slots select running guests. |
| `proxmox lab-pve pct enter ` | Container IDs on the chosen SSH node | Reads inventory over SSH using the configured alias. |
| `pct rollback 100 `, `qm migrate 410 ` | Snapshot names or cluster migration nodes | Reads the selected node's guest or cluster inventory. |
| `pvesm list `, `pvesm path fast:` | Storage IDs or volumes in the specified storage | Reads storage on the local or explicitly selected SSH node. |
| `zfs rollback tank/app@`, `zpool status ` | Snapshots or pool names | Reads the local or explicitly selected SSH node. |

The Docker Compose and Docker Engine providers are separate. Compose reads project configuration,
while container and image lookups use engine state. GitHub and Kubernetes can contact the currently
configured service when completing a resource. The standard live-query deadline is 350 ms per
invocation. GitHub gets 850 ms for a fresh listing and reuses successful results for 20 seconds.
Its cache is scoped to repository, command, working directory, and GitHub CLI context. Cache files
are private to the local account.

Docker and Podman command names, nested command paths, and option spellings can also come from the
installed CLI's help. Live candidates take precedence in slots with a matching provider. The
shell's completion remains available for other arguments.

`docker run --mount=type=volume,source=` offers named volumes, and `docker ps --filter=ancestor=`
offers local images. Filters are interpreted in the active command's grammar. Docker container
updates can prefill the selected container's current `--restart` policy. In Compose,
`up --scale=` inserts a service name followed by `=` so you can supply the replica count. Compose
`cp` can insert a service name with `:` for the container side of the path. Podman completes
`--pod`, `--secret`, and machine names. Secret lookup queries names and identifiers.

For `git config set`, Tab reads the same config scope as the pending write. A write without a
scope flag targets local config. The legacy `git config --global user.email ` spelling also
prefills the current value. Prefill is limited to common scalar preferences such as `user.email`,
`user.name`, `init.defaultBranch`, `core.autocrlf`, and `push.default`. Credential and
command-bearing values stay out of COMPLETE-ME's config suggestions. Diagnostics report the
lookup outcome and scope without printing the value.

### Proxmox and ZFS

Add the aliases you already use with OpenSSH to
`${XDG_CONFIG_HOME:-$HOME/.config}/complete-me/proxmox-hosts`, one alias per line. Blank lines and
`#` comments are supported. Each entry is a single alias, such as `lab-pve`. SSH options such as
`HostName`, `User`, and `ProxyJump` come from your SSH configuration. The SSH account needs
permission to run the relevant Proxmox commands.

```text
proxmox <Tab>
proxmox lab-pve pct enter <Tab>
proxmox lab-pve qm status 41<Tab>
proxmox lab-pve qm migrate 410 <Tab>
proxmox lab-pve pvesm path fast:<Tab>
proxmox lab-pve zfs rollback tank/app@<Tab>
```

`proxmox` is a shell function installed with the Bash, zsh, and fish integrations. You can also call
`complete-me proxmox lab-pve …` directly. A host by itself opens an SSH login session.
Submitting `pct enter`, `pct console`, `qm terminal`, or `qm monitor` requests a PTY from SSH.
Commands run on the alias you chose. Add each cluster node you work with and choose the node that
owns the guest.

For cluster-wide selection, add a line to
`${XDG_CONFIG_HOME:-$HOME/.config}/complete-me/proxmox-clusters` with a unique cluster name, an SSH
alias to query for inventory, then one Proxmox-node-name to SSH-alias mapping per reachable node.
All SSH aliases must already appear in `proxmox-hosts`.

```text
lab pve-a pve01=pve-a pve02=pve-b
```

Type `proxmox lab pct enter <Tab>` or `proxmox lab qm terminal <Tab>`. Fish and zsh show the
guest's name, state, and owning node. On submission, COMPLETE-ME queries the cluster again and uses
the owning node's configured SSH alias. The execution connection requires a trusted SSH host key
and checks the remote short hostname against the mapping. Place the guest ID directly after the action, including
after grouped actions such as `qm guest cmd`. Duplicate guest IDs and guests with unmapped nodes
are excluded. For node-local commands such as `pvesm` and `zfs`, use an individual SSH alias or
select a mapped node with `proxmox lab --node pve02`. An SSH login uses the same terminal handling
described above.

Tab uses bounded, read-only inventory commands over a noninteractive SSH connection. Remote
lookups require working noninteractive authentication and a trusted host key, and allow up to
1.4 seconds per query. Use `complete-me live --explain --line 'proxmox lab-pve pct enter 10'` to
inspect the selected node and lookup outcome.

Export `COMPLETE_ME_PROXMOX_IP=1` to include addresses when one guest ID matches. LXC
addresses come from static network configuration. QEMU addresses come from the running VM's guest
agent when available. The ID is the text inserted into your command, and fish and zsh can display the
name, state, node, and address alongside it. Bash's Readline menu shows IDs. Run
`complete-me live --line 'proxmox lab-pve qm status 41'` to display descriptions in any shell.

On a machine with a mounted ZFS dataset, use `complete-me zfs-path tank/app logs/<Tab>` to
browse relative paths in that dataset. Submitting the command prints the resolved local path.
The `zfs-path` shell shortcut works on any machine with OpenZFS. Give a filesystem name such as
`tank/app` or a mounted pool root such as `tank` to print its mountpoint. The helper also resolves
legacy mounts through the local mount table. Path completion follows existing directories inside
the mount and excludes paths that resolve outside it.

#### Backup and restore

```sh
vzdump <Tab>
vzdump 410 --storage <Tab>
qmrestore vault:<Tab>
qmrestore vault:backup/vm/410/2026-11-19T05:30:00Z <Tab>
pct restore <Tab>
pct restore 510 vault:<Tab>
proxmox lab --node pve02 qmrestore vault:<Tab>
```

`vzdump` lists guests on the execution node and offers configured storage that accepts backups. At the
archive position, restore commands first list eligible stores. After selecting `STORAGE:`,
`qmrestore` lists VM archives and `pct restore` lists container archives. Directory-style backup
archives and Proxmox Backup Server volume IDs are supported. Directory lookups use the configured
path on the node and require an existing mount for network filesystems. Descriptions include the
original guest ID, the current name associated with that ID if available, the archive timestamp,
and the storage ID. `--storage` offers configured image storage for VMs or root directory
storage for containers. The next guest ID is advisory. Proxmox validates that ID when you submit
the restore. Tab queries inventory. Enter submits the native backup or restore command.

### Disk, byte, and network helpers

| Example | What Tab offers |
|---|---|
| `dd if=/dev/` or `dd of=/dev/` | Linux devices and partitions with geometry, size, and mount state. Read-only devices are omitted from the output list. |
| `dd if=/dev/sda skip=` or `dd of=/dev/sdb seek=` | Partition starts relative to the selected whole disk. On a selected partition, zero is the relative start. |
| `dd if=/dev/sda ibs=` or `dd of=/dev/sdb obs=` | Logical and physical sector sizes for the respective device. `bs=` can use either side. |
| `hexdump -C -s ` with a literal filename to the right | Structural landmarks and printable-region offsets in that file. |
| `tcpdump -i `, `tshark -i `, `wireshark -i ` | Local interface names. TShark and Wireshark also suggest common `-f` capture and `-Y` display filter terms. |
| `ip link show dev `, `ip route show table `, `route add dev ` | Local interface names or standard route table names, depending on the slot. |
| `nmap -o` | Report output flags. The provider also offers help, version, and interface listing flags. |

The `dd` provider uses `lsblk` metadata. Linux partition starts use 512-byte sectors, so completion
converts them to byte offsets and inserts GNU `dd`'s `B` suffix. This keeps offsets independent of
`bs`, `ibs`, and `obs`, including when those operands are to the right of the cursor. GNU `dd` 9.1 or
later supports this suffix. The actual input and output operands are `skip=` and `seek=`.

`cm-bytes FILE` reports offsets in hex and decimal and describes each landmark. It examines up to
the first 1 MiB of the named local regular file. For a specific text match, `rg -a -b -o PATTERN FILE`
reports its byte offset. Use that offset with `hexdump -C -s OFFSET FILE`. When inspecting an
imported binary in Ghidra, compare file offsets with the program's memory map before navigating
to the corresponding loaded address.

### Installed shortcuts

Bash, fish, and zsh install `proxmox`, `kube`, `cm-cloud`, `zfs-path`, and `path-why` plus these shortcuts for the
corresponding `complete-me` subcommands. Your existing definitions take precedence.

`cm-alias`, `cm-bash2fish`, `cm-bytes`, `cm-doctor`, `cm-env`, `cm-explain`, `cm-find`,
`cm-forget`, `cm-func`, `cm-gen`, `cm-list`, `cm-live`, `cm-path`, `cm-recipe`, `cm-regex`,
`cm-show`, `cm-snippets`, and `cm-ssh-options`.

### Cloud and agent providers

`kube CONTEXT NAMESPACE COMMAND …` runs kubectl with an explicit context and namespace. Select
`@all` in the namespace slot to list qualified names such as `team-a/web`. The shortcut routes
`logs`, `exec`, `attach`, and `port-forward` for pods, and `get` or `describe` for a single named
resource, to the selected namespace. After selecting a pod, complete `-c` or `--container` to see its container names and images. Exec arguments after `--` pass through unchanged. Use an
explicit namespace for other kubectl commands. `kubectl` itself keeps its standard syntax.

| Provider | Examples | Data source |
|---|---|---|
| AWS CLI | `aws --profile=`, `aws --profile work eks update-kubeconfig --name `, `aws s3 cp s3://example-bucket/logs/` | Local profiles and live inventories in the selected account and region. Covers EC2 instances, ECS clusters, Lambda functions, RDS instances, and S3 buckets, objects and prefixes. |
| Vultr CLI | `vultr-cli instance get `, `vultr-cli instance create --region=`, `vultr-cli dns domain get ` | Instances, regions, DNS domains, and Kubernetes clusters in the configured Vultr account. |
| Lambda Cloud | `cm-cloud lambda instances ` | Instance ID with name, status, region and IP. Set `COMPLETE_ME_LAMBDA_API_KEY` in the environment. |
| DreamHost | `cm-cloud dreamhost records ` | DNS record names and types. Set `COMPLETE_ME_DREAMHOST_API_KEY` to a key scoped to `dns-list_records`. |
| Hugging Face | `hf download `, `hf cache rm ` | Local cache inventory, including `--cache-dir` and `--repo-type`. |

The AWS and Vultr providers use the installed CLI and its authenticated configuration. Cloud API
completion sends credentials to `curl` over stdin. DreamHost's API requires its key in the
HTTPS request query. Completion output and diagnostics omit that key and DNS record values.
`COMPLETE_ME_NO_REMOTE=1` keeps local profiles, kubeconfig contexts, Hugging Face cache entries,
and agent metadata available while disabling the new remote lookups.

S3 object tracking browses the prefix you type and shows object sizes and last-modified times
when available. Use `aws --profile work s3 cp s3://example-bucket/logs/<Tab> ./download`,
`aws s3api get-object --bucket example-bucket --key logs/<Tab> ./download`, or
`aws s3api list-objects-v2 --bucket example-bucket --prefix logs/<Tab>`. Each object lookup makes
one bounded `ListObjectsV2` request for up to 50 objects and common prefixes. Type more of the
key to narrow a busy prefix. The selected profile, region, and explicit `--expected-bucket-owner`
apply to the lookup, including selectors after the cursor. The provider requires bucket listing
access and performs a fresh listing when you request completion. AWS inventory uses standard AWS
endpoints and the credential provider configured in the trusted login environment. Completions
skip custom endpoints and per-command overrides of the AWS configuration or credentials files.
In fish, use COMPLETE-ME's Tab binding for object lookups. Fish's native completion functions
receive a truncated view of the command and apply their own settings.

The bundled agent grammar covers `codex`, `claude`, `gemini`, `agy`, `hermes`, `cline`,
`opencode`, `aider`, `cursor-agent`, `copilot`, `droid`, `qwen`, `goose`, `amp`, and `pi`.
`fx` has an extension entry because the executable name is shared by different tools. Codex
session candidates use local IDs and dates from its session index when present. Claude Code
session candidates use local project filenames. These providers avoid reading conversation
bodies for candidates.

Create `${XDG_CONFIG_HOME:-$HOME/.config}/complete-me/agents.json` for machine-specific values.
Each key names an executable. Supported entries include `commands`, `models`, `sessions`, and
per-flag `values`, with descriptions supplied as strings.

```json
{
  "codex": {
    "models": {"lab/model": "Local model"},
    "values": {"--profile": {"lab": "Lab profile"}}
  },
  "opencode": {"sessions": {"ses_example": "Workspace session"}},
  "fx": {"commands": {"models": "Local models"}}
}
```

`complete-me gen --force TOOL` generates completion definitions for a custom agent declared in
that file. The catalog stores display names and IDs. Use the agent's normal authentication for
credentials.

### Inspect live coverage

```sh
complete-me live --coverage
complete-me live --coverage --json podman
complete-me live --coverage proxmox
complete-me live --explain --json --line 'docker run --mount=type=volume,source='
```

The coverage catalog lists supported families, examples, and the data source even on an empty
runtime. `--explain` checks a particular cursor position on the installed tool and reports the
lookup outcome. The catalog also records limits such as registry-only tags, remote-only Git refs,
and filesystem paths inside a container. Those positions can still use other shell completions
where available. Plugin-defined options and arbitrary input values vary by CLI and version.

## Select the runtime

Live queries honor existing command flags and supported literal environment selectors, including
`DOCKER_HOST`, `DOCKER_CONTEXT`, `COMPOSE_FILE`, `KUBECONFIG`, `GH_HOST`, and `GH_REPO`.
You can set them in the shell or on the command being edited. The provider passes relevant
settings to its child process only.

```sh
complete-me live --line 'env -u DOCKER_HOST DOCKER_CONTEXT=work docker logs '
complete-me explain --completion --json --line 'GH_HOST=github.com GH_REPO=arf-io/COMPLETE-ME gh pr view '
```

The resolver recognizes common wrappers such as `env`, `command`, `time`, and `nice`.
It reports a blocked live scope for identity changes, unsupported wrapper options, or selectors
that require shell evaluation. Reports show the names of environment selectors and omit their
values. GitHub cache entries include the effective GitHub connection context.

Set `COMPLETE_ME_NO_REMOTE=1` to disable COMPLETE-ME's GitHub, Kubernetes resource, and Proxmox SSH completion queries.
Kubernetes context names, Git refs, tmux targets, manifests, and other local providers remain
available. In this mode Docker and Podman engine queries require an explicitly local Unix socket.
Compose project queries are also disabled because Compose files can include remote projects.
Shell-native or third-party completion scripts apply their own settings.

## Explain a completion

Pass the text from the start of the current command through the cursor. A trailing space represents
an empty argument.

```sh
complete-me explain --completion --line 'tmux -L dev a -t '
complete-me live --explain --json --line 'gh pr view -R arf-io/COMPLETE-ME '
complete-me live --current web -- docker compose logs
```

The report shows the provider, recognized slot when available, selected context or repository, query
outcome, and candidate count. `--json` returns the same information as structured data. A cache hit,
a missing tool, a timeout, a failed command, and a successful query with zero matching candidates
appear as distinct outcomes. The report omits raw subprocess errors and sensitive arguments.
If a slot has no live matches, ordinary shell completion can still apply.

## Inspect your installed shell

```sh
complete-me doctor --command cargo --json
complete-me doctor --line 'tmux -L work a -t '
```

Run `doctor` through the sourced fish, Bash, or zsh integration to see the live Tab binding and
which handler the shell selected. The report shows whether the generated cache exists and whether
its handler is loaded. With `--line`, it also runs a live lookup and reports its duration.
Invoke `command complete-me doctor --shell fish` to inspect on-disk state directly. Direct
invocation reports parent-shell binding information as unknown.

## Manage fish functions and PATH

```fish
complete-me alias --list
complete-me func --list
complete-me func --save my_function
complete-me func mkcd 'mkdir -p $1; and cd $1'
complete-me path list
complete-me path add ~/.local/bin
```

The last command delegates to `fish_add_path` and accepts its options. Functions created through
`complete-me func` are loaded in the running fish session and saved for future sessions.

Run `complete-me path why python` through the loaded fish, Bash, or zsh hook to see the selected
alias, function, builtin, or executable, followed by each executable on `PATH` in search order.
The report also identifies repeated directories. The standalone binary can inspect the inherited
`PATH`. Shell definitions and cached command lookups come from the hook.

## Environment changes

`complete-me env diff FILE` compares a file with the current exported environment. It prints the
names it would add, change, or remove. In fish, `complete-me env apply FILE` previews those changes
and prompts before applying them. Use `apply --yes FILE` to confirm up front and `env undo` to
restore the last applied change in that fish session. Undo is stored in memory and restores values,
list elements, and export and path attributes. Edits target global variables in the running fish
session. Universal variables and fish internals are protected.

The input is UTF-8 and uses one `NAME=value`, `export NAME=value`, or `unset NAME` per line.
Blank lines and comments are allowed. Use quotes for spaces and punctuation. Double quotes accept
`\n`, `\r`, `\t`, `\\`, `\"`, and `\$` escapes. Values are literal, including `$HOME` and command
substitution text inside quotes. Later entries for the same name take precedence.

```sh
complete-me env diff - <<'EOF'
export APP_LABEL="Development server"
APP_PROFILE=local
unset OLD_PROFILE
EOF
```

In fish, `complete-me env apply --yes - < .env.local` reads the same format from stdin.

## Project recipes

Save literal arguments in `.complete-me/recipes.json`. COMPLETE-ME finds the nearest file by
walking up from the working directory. Live lookups and the final command use the current working
directory.

```json
{
  "logs": ["docker", "compose", "logs", "-f", "{service}"],
  "switch": ["git", "switch", "{branch}"]
}
```

- `complete-me recipe list` lists available recipe names and their templates.
- `complete-me recipe show logs` shows the template and the order of its slots.
- `complete-me recipe expand logs worker` prints a shell-escaped command for review.

A slot occupies one argument and has a name such as `{service}`. Give it a literal value. Reusing
the same slot in a template reuses its value. After `complete-me recipe expand logs `, Tab calls the
existing Compose provider for service names. Other supported commands use their own live providers,
with the same scope and offline settings. For a slot without live matches, type the value yourself.
Once every slot has a value, **Alt+Shift+R** in fish, Bash, or zsh replaces the standalone recipe
invocation with the quoted command in the editor. Review it, then press Enter to run it.

## Calculator

Enter an arithmetic expression ending in **=** and press **Tab**. For example, **2*34=**
becomes **2*34=68** in the editor. Use **ans** for the latest Tab result in that shell session.
Clear the working line with **Ctrl+C** when you want a fresh prompt.

To chain calculations on one line, leave a space after the answer and type an opening
parenthesis. Press Tab there to insert the previous result. Continue with **/3)^2=** and
press Tab again. In a chain, Tab also inserts the last answer after an operator or comma.
A negative answer is parenthesized when inserted as an operand.

Run standalone calculations with:

    complete-me calc 'sqrt(144) + log(1000, 10)'
    complete-me calc 'round(sin(rad(90))) + hypot(3, 4)'
    complete-me calc --last 68 '(ans/3)^2'
    complete-me calc '10! / (3! * 7!)'

The calculator uses double-precision arithmetic. Power accepts **^** or **\*\*** and binds
right-to-left. Division, modulo, and multiplication share precedence. Factorial binds before
power. Trig takes radians, with **rad(x)** and **deg(x)** for conversion. Constants include
**pi**, **tau**, and **e**. Scientific notation, square and cube roots, logarithms, rounding,
and two-argument min/max and hypotenuse are supported. The standalone command reports invalid,
undefined, or out-of-range results. Expressions are evaluated as numeric input. Set
**COMPLETE_ME_NO_CALC=1** before loading the shell hooks to disable calculator Tab handling.
