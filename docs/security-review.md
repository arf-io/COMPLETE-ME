# Independent security review of Proxmox backup and S3 completion

The initial static review examined `3fcdc9ec10cf8e69ccd455fca0486dbfe40f1f41` against
`f6e3a74`. A separate Codex CLI session running GPT-6.2-Sol read an exported source snapshot.
The reviewer had a read-only remit, used source inspection, and received the change and its
verification scope without the implementation conversation or production account data. Review
coordination used tmux and the Party Line. This repository record captures the findings and
their disposition.

| Severity | Finding | Remediation and evidence |
|---|---|---|
| Medium | S3 lookups could miss a profile, region, bucket, or expected owner to the right of the cursor. | Read both sides of the token within the same command. Shell editor tests exercise the trailing profile in Bash, zsh, and fish. Fixture tests cover the bucket, region, and expected owner. |
| Medium | The AWS CLI could use custom endpoints and command-local credential configuration on Tab. | Inventory declines detected custom destinations and inline configuration-file overrides. The subprocess also ignores configured endpoint URLs. Tests verify that skipped lookups launch no AWS CLI. |
| Medium | Command-line text reached helper process arguments, and history suggestions could include restore passwords. | Shell Tab adapters send editor text over stdin. Password-bearing container restores and creates are excluded from history suggestions, with the password completion slot blocked. |
| Medium | Bash could expand an exclamation point in a completed key inside double quotes. | Suppress the value in that quoting context. Tests cover Bash rendering and preserve ordinary S3 keys containing spaces across all three editors. |
| Medium | A stale alias could route a restore to a different Proxmox node, and the remote login shell could alter quoted arguments. | SSH requires a known host key. A mapped node is checked on the execution connection before a command proceeds. Shell round-trip tests cover spaces, apostrophes, and backslashes. A mismatched node test verifies that the restore fixture stays idle. |
| Medium | Subprocess cleanup could wait for descendants holding pipes after the deadline. | Capture uses bounded nonblocking reads and writes. Persistent descriptors cannot hold up the caller. Process timeout and stdin regressions run in the verification suite. |
| Low | A reused guest ID could make a backup appear to belong to the current guest. | The archive menu labels the value as the current guest name and displays the original ID separately. |

The implementation review also found that a storage status lookup can activate a Proxmox storage
backend. Backup destination suggestions read storage configuration. Directory archive browsing
uses read-only filesystem listing and gates network stores on an existing mount. PBS browsing
uses its datastore inventory. See [Proxmox storage discovery and activation](https://github.com/proxmox/pve-storage/blob/master/src/PVE/Storage.pm)
and the [PBS backend activation code](https://github.com/proxmox/pve-storage/blob/master/src/PVE/Storage/PBSPlugin.pm).

## Follow-up

The reviewer checked `88ad3a1af72ccdf28612f214779e5e1d98177264` and identified four remaining
cases. Commit `307c5c2ee98dc7ef5b904c223562845bc975dd1c` addresses them.

| Severity | Follow-up finding | Remediation and evidence |
|---|---|---|
| Medium | Native fish completion has a truncated view of the command and can lose trailing S3 selectors. | COMPLETE-ME's native fish adapter skips S3 object queries. The COMPLETE-ME Tab binding uses the full buffer. The PTY regression exercises both paths. |
| Medium | A fish completion fragment could create a process expansion when inserted as a standalone token. | Fish escapes the trigger in new text and withholds the candidate if the already-typed text prevents a literal edit. A unit test covers both cases and quoted values. |
| Medium | A directory lookup could encounter a symlink or automount even after checking the configured storage mount. | The Linux reader opens each directory without following symlinks or triggering automounts. Directory descriptors pin the mounts while the reader checks their live identities. The reader skips stacked mounts, mounts below the backup storage root, and network archives in offline mode. Linux tests exercise real directory reads and symlink rejection. |
| Low | Directory archive completion could offer a notes sidecar. | Directory candidates match exact native archive and compression extensions. Fixtures include a notes sidecar and an archive name with a trailing dot. |

The follow-up review confirmed both fish fixes and identified two refinements for the directory
reader. Commit `7bfcd490c3616620a2ad38e23b2645a1fea09e54` rejects malformed suffixes and uses
Linux `O_PATH` for discovery. Commit `aaf9bc6800ffebdb9a622197cd4da8fc3f9a073c` pins crossed
mounts, rejects ambiguity, and checks the mount's live device and source before advancing. See
the [Linux open documentation](https://man7.org/linux/man-pages/man2/open.2.html).
The reviewer rechecked the final mount change and closed the stacked-mount finding.

## Verification boundary

The review covers the changed completion and submission paths and their shared rendering,
credential scope, and process-capture code. The automated evidence uses fixtures and real shell
editors. The AWS CLI's installed credential chain and OpenSSH configuration are part of the
operator's trusted environment. For example, a configured AWS credential helper can run to
authenticate an inventory request. Restore IDs and archives are suggestions from lookup time.
Proxmox checks availability again when a command is submitted. Authenticated PVE and PBS
checks, IAM behavior in a disposable AWS account, and a comprehensive application security
assessment remain open.
