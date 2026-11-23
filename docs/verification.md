# Provider verification and security review

The public verification workflow records the exact Git commit, compiler, coverage tool, and shell
versions alongside the LLVM coverage export and a Markdown summary. The regular Linux and macOS
jobs run shell syntax checks, the release suite, and Clippy. The real-services workflow exercises
Docker, Podman, a disposable Kubernetes cluster, and the Linux help sandbox. See [testing](testing.md) for the commands and runners.

## Reproduce the coverage report

Install `cargo-llvm-cov` 0.8.7 and Rust nightly 2026-11-21 with `llvm-tools-preview`. From the
repository root, run:

```sh
mkdir -p target/verification
RUSTC="$(rustup which --toolchain nightly-2026-11-21 rustc)" \
RUSTDOC="$(rustup which --toolchain nightly-2026-11-21 rustdoc)" \
COMPLETE_ME_REQUIRE_TEST_TOOLS=1 \
  rustup run nightly-2026-11-21 cargo llvm-cov --locked --branch \
    --ignore-filename-regex '(^|/)(rustlib|tests)/' \
    --json --output-path target/verification/coverage.json
python3 tools/coverage_report.py target/verification/coverage.json
```

Use `--baseline PATH` with an LLVM export from the prior revision to report the change in
percentage points and counts. The summary includes instrumented Rust lines, branch outcomes,
functions, and regions. The Linux verification job currently requires at least 79.5% line and
60.5% branch coverage. Shell behavior is verified through the shell matrix and real terminal
editor tests.

## Evidence by boundary

| Boundary | Automated evidence | Live verification still needed |
|---|---|---|
| Proxmox inventory and routing | Tests assert the exact read-only query, apply node mappings, reject unmapped and duplicate guest IDs, preserve PTY arguments on submission, and honor offline mode during completion. | Two Proxmox nodes, a guest migration between lookup and submission, and a console round-trip. |
| Proxmox backup and restore | Tests distinguish VM and container archives, validate volume IDs and storage filters, and verify node-scoped backup inventory and the next restore ID. Linux also exercises the directory reader against real files and symlinks. | Read-only inventory checks on a two-node cluster with directory and Proxmox Backup Server storage. |
| Kubernetes namespaces | Tests use duplicate names across namespaces, route the selected pod with explicit context and namespace, preserve command arguments, and reject conflicting selectors and secret inventory. The real-services job exercises duplicate pod names in two namespaces. | Repeat against a restricted account with denied namespace access. |
| AWS inventory | Tests verify the fixed CLI operation for EC2, EKS, ECS, Lambda, RDS and S3, profile/region forwarding, and offline mode. | A disposable account or sandbox profile to verify permissions, current CLI response formats, and a cold-auth timeout. |
| S3 objects and prefixes | Tests verify the single bounded listing, bucket and expected-owner scope, offline and unsupported selector behavior, and insertion of an object with a space in Bash, zsh, and fish. | Verify listing and denied access with a disposable bucket and a restricted AWS profile. |
| Vultr, Hugging Face, Lambda Cloud, and DreamHost | Tests verify strict query arguments, offline cache behavior, credentials on stdin, and withholding sensitive DNS values. | Account-backed read checks and shaping against supported CLI versions. |
| Agent integrations | Tests register fake executables that fail if launched, exercise model and session lookup and generated completions, and check that transcript titles stay out of results. | A version matrix against installed agent CLIs and fresh user configuration. |
| Rendering and process lifetime | Bash, zsh and fish tests verify insertion and quoting, source generated shell definitions, and assert subprocess cancellation. | Include PTY and multi-shell cases in the independent review. |

## Independent red-team review

The [November 21, 2026 source review](security-review.md) covers Proxmox backup and restore, S3
object completion, and the shared boundaries they use.

Begin with the commit and artifacts from a successful verification run. Use a clean workspace,
synthetic credentials, and disposable resources. Obtain authorization for each external test
account or cluster used in the review. Keep raw credentials, real session contents, and inventory
from production accounts out of the review artifacts.

Have reviewers independently examine shell parsing and rendering, scope selection, credential
handling, process cancellation, and the help-probe sandbox. Check whether each advertised
read-only operation stays within the configured destination and identity. Include malformed
configuration, unavailable services, and stale or ambiguous resource ownership. Validate installed
shell hooks in Bash, zsh, and fish and observe whether invoking Tab can change runtime state.

Record the audited commit, fixture or authorized environment, expected behavior, observed behavior,
and affected version for each finding. Land regression tests with fixes and re-run the same
verification workflow. The automated suite and its coverage report are inputs to this review.
