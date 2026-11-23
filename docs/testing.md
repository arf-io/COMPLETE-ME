# Testing COMPLETE-ME

Run the same checks used by the public Linux and macOS test jobs.

```sh
make check
COMPLETE_ME_REQUIRE_TEST_TOOLS=1 cargo test --locked --release
cargo clippy --locked --release --all-targets -- -D warnings
```

The full test suite requires fish 4+, Bash 4.2+, zsh, tmux, Python 3, Git, SSH, and the
`ssh_config` manual. For coverage of Bash's native integration, install Bash completion as well.
With `COMPLETE_ME_REQUIRE_TEST_TOOLS=1`, missing required tools fail the suite. Ordinary local
`cargo test --locked` permits platform-specific tests to skip unavailable tools.

| Test layer | What runs |
|---|---|
| Parser, renderer, and translation snapshots | Saved help from real CLIs, generated fish and Bash scripts, and the Bash-to-fish corpus. |
| Shell parsing and generated completions | Real fish, Bash, and zsh exercise the same nested alias, option-cluster, and argument-value fixture. |
| Interactive editor tests | Real Tab keys verify insertion before later arguments, including SSH paths with spaces. Fish Enter tests cover Bash exports. Recipe tests complete live slots and use Alt+Shift+R to expand a command in the editor. |
| Live state | An isolated tmux server changes windows between queries. A temporary Git repository supplies refs from a selected worktree. |
| Provider contracts | Executable fixtures check the exact read-only commands for Docker, Podman, Compose, ZeroTier, GitHub CLI, Kubernetes, systemd, Just, Homebrew, Proxmox, and ZFS. Project manifests are read from temporary files. |
| Diagnostics and cache | Tests cover candidate filtering, unsupported slots, failure status, timeouts, redaction, cache permissions, scope isolation, and expiry. |
| Environment and PATH | Fish tests check preview redaction, literal values, apply and undo, and rejection of an invalid file. Fish, Bash, and zsh tests check aliases, builtins, shadowed executables, and repeated PATH entries. |

Provider fixtures let the core suite run without a cluster, container daemon, or GitHub login.
They verify how COMPLETE-ME constructs queries and handles the responses. Availability and results in
a real session depend on the installed CLI, authenticated account, and runtime.

The Proxmox tests check guest metadata, host selection, and mid-line Tab in Bash, fish, and zsh.
A fixture verifies that completion uses read-only SSH commands and that submitting `pct enter`
asks SSH for a PTY. ZFS tests check dataset, snapshot, and pool values, plus scoped path lookup
through a temporary directory with an escaping symlink.

Disk fixtures verify sector-to-byte conversion, device-specific block sizes, input versus output
offsets, and filtering of read-only destinations. Network tests use the host's loopback interface.
Shell editor tests exercise byte-offset completion before a file argument. Shortcut tests verify
that existing user definitions take precedence. The fish suite also loads a vendor completion that
erases earlier container registrations and verifies that native arguments and live metadata coexist.

GitHub Actions checks the workflow files with `actionlint` and runs the test matrix on Linux and
macOS. The browser demo workflow runs on site changes in pull requests and pushes to `main`.
It checks the recorded shell comparison in Chromium,
including Tab markers, replay, offline example switching, and the narrow layout. The existing
Forgejo workflow runs on its configured host runner.

To refresh the generated fish and Bash snapshots after an intentional renderer change, run:

```sh
COMPLETE_ME_UPDATE_RENDER_GOLDENS=1 cargo test --locked --test golden_core
git diff -- tests/golden
```

The parser and translation snapshots remain independent of that update command.


## Real services and sandbox

The separate GitHub Actions workflow runs on relevant pull requests and pushes to `main`. It can
also be started manually.
Its Docker job uses the hosted runner's disposable daemon and a unique Compose project. Tests
check names and metadata while a service is running, then confirm that stopping it changes
the offered candidates. It also verifies Compose service options, named volume matches, and a
container's current restart policy. The Podman job uses disposable rootless storage and checks pods,
volumes, networks, and secret names. The Linux sandbox job asserts the real bubblewrap probe sees a read-only fixture,
a private temporary directory, and a distinct network namespace. Generated fish, Bash, and zsh
fixtures are also sourced in the regular suite to check that description text stays inert.

To run the opt-in checks locally, supply a disposable Docker daemon or use Linux with bubblewrap
and user namespaces available.

```sh
COMPLETE_ME_TEST_DOCKER=1 COMPLETE_ME_TEST_DOCKER_HOST=unix:///var/run/docker.sock \
  cargo test --locked --test real_services docker_and_compose_follow_real_container_state

COMPLETE_ME_TEST_SANDBOX=1 \
  cargo test --locked --test real_services linux_help_probe_uses_read_only_files_and_private_namespaces

COMPLETE_ME_TEST_PODMAN=1 \
  cargo test --locked --test real_services podman_reads_disposable_pods_volumes_networks_and_secret_names
```

## Rust coverage

With `cargo-llvm-cov` and the compiler's matching LLVM tools installed, measure the main suite:

```sh
COMPLETE_ME_REQUIRE_TEST_TOOLS=1 cargo llvm-cov --locked \
  --ignore-filename-regex '(^|/)(rustlib|tests)/'
```

The [verification workflow and review guide](verification.md) specify the nightly branch-coverage
command, the evidence artifacts, and the provider-specific checks. The percentages cover
instrumented Rust. The editor and shell scripts are checked through the separate terminal and
syntax tests.
